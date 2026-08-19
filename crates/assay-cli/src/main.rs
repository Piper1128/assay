//! The `assay` binary — the only crate that may write to stdout
//! (ADR-000 rev 2).
//!
//! Subcommands exist when they work. `diff` needs two dataset versions to
//! say anything; with only one committed it reports that plainly rather than
//! printing an empty result that reads like "nothing changed".

mod loadout_file;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use assay_core::confidence::ConfidenceLevel;
use assay_core::exchange::{Exchange, ExchangeContext, Strike};
use assay_core::resolve::Resolved;
use assay_core::schema::DatasetSource;
use assay_core::{Confidence, canonical_statblock, resolve};

const USAGE: &str = "\
assay — headless stat resolver and patch differ for Dark and Darker

USAGE:
    assay resolve <loadout.toml> [OPTIONS]
    assay exchange <attacker.toml> <defender.toml> [OPTIONS]
    assay diff <build-a> <build-b> [--loadouts <dir>] [--data <dir>]
    assay versions [--data <dir>]

RESOLVE OPTIONS:
    --build <id>     dataset build to resolve against (default: the newest
                     committed build)
    --data <dir>     dataset root (default: ./data)
    --explain        print every pipeline stage: input, transformation, output
    --json           print the canonical form instead of a table
    --strict         exit 2 if any value is below `verified` (ADR-007)

EXCHANGE OPTIONS:
    --explain        print all nine damage steps (ADR-006)
    --build / --data as for resolve

DIFF OPTIONS:
    --loadouts <dir>  also resolve every .toml in <dir> against both versions
                      and report the stat deltas (ADR-008 level 2)

Confidence is shown on every number, because a tool that cannot tell a
measured value from a guess makes you act on guesses with confidence:
    ok   verified    confirmed in patch notes or by an in-game test
    ~    unverified  wiki/community sourced, not confirmed
    ?    unknown     an assumption; the reason is listed below the table
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("assay: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    match args.first().map(String::as_str) {
        Some("resolve") => cmd_resolve(&args[1..]),
        Some("exchange") => cmd_exchange(&args[1..]),
        Some("versions") => cmd_versions(&args[1..]),
        Some("--help" | "-h" | "help") | None => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some("diff") => cmd_diff(&args[1..]),
        Some(other) => Err(format!("unknown command: {other}\n\n{USAGE}")),
    }
}

/// Flags parsed off an argument list, so an unknown flag is an error rather
/// than something silently ignored.
struct Flags {
    positional: Vec<String>,
    data: PathBuf,
    build: Option<String>,
    loadouts: Option<PathBuf>,
    explain: bool,
    json: bool,
    strict: bool,
}

fn parse_flags(args: &[String], allowed: &[&str]) -> Result<Flags, String> {
    let mut flags = Flags {
        positional: Vec::new(),
        data: PathBuf::from("data"),
        build: None,
        loadouts: None,
        explain: false,
        json: false,
        strict: false,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let needs_value = |name: &str| -> Result<String, String> {
            iter.clone()
                .next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            flag if flag.starts_with("--") && !allowed.contains(&flag) => {
                return Err(format!("unknown option: {flag}"));
            }
            "--data" => {
                flags.data = PathBuf::from(needs_value("--data")?);
                iter.next();
            }
            "--build" => {
                flags.build = Some(needs_value("--build")?);
                iter.next();
            }
            "--loadouts" => {
                flags.loadouts = Some(PathBuf::from(needs_value("--loadouts")?));
                iter.next();
            }
            "--explain" => flags.explain = true,
            "--json" => flags.json = true,
            "--strict" => flags.strict = true,
            positional => flags.positional.push(positional.to_string()),
        }
    }
    Ok(flags)
}

fn cmd_versions(args: &[String]) -> Result<ExitCode, String> {
    let flags = parse_flags(args, &["--data"])?;
    let versions = assay_data::versions(&flags.data).map_err(|e| e.to_string())?;
    if versions.is_empty() {
        return Err(format!(
            "no dataset versions under {}",
            flags.data.display()
        ));
    }
    for build in &versions {
        let dataset = assay_data::load(&flags.data, build).map_err(|e| e.to_string())?;
        println!(
            "{build}  {}  released {}",
            dataset.manifest.label, dataset.manifest.released
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_resolve(args: &[String]) -> Result<ExitCode, String> {
    let flags = parse_flags(
        args,
        &["--data", "--build", "--explain", "--json", "--strict"],
    )?;
    let [path] = flags.positional.as_slice() else {
        return Err("resolve needs exactly one loadout file".to_string());
    };

    let build = match &flags.build {
        Some(build) => build.clone(),
        None => newest_build(&flags.data)?,
    };
    let dataset = assay_data::load(&flags.data, &build).map_err(|e| e.to_string())?;

    let loadout = read_loadout(path)?;

    let resolved = resolve(&loadout, &dataset.entities).map_err(|e| e.to_string())?;

    if flags.json {
        println!("{}", canonical_statblock(&resolved));
    } else {
        print_table(&loadout.name, &build, &dataset.manifest.label, &resolved);
    }
    if flags.explain {
        print_explain(&resolved);
    }

    if flags.strict {
        let below: Vec<&str> = resolved
            .derived
            .iter()
            .filter(|(_, value)| value.level() != ConfidenceLevel::Verified)
            .map(|(id, _)| id.as_str())
            .collect();
        if !below.is_empty() {
            eprintln!(
                "assay: --strict: {} value(s) below verified: {}",
                below.len(),
                below.join(", ")
            );
            // Distinct from 1 so a script can tell "unverified data" from
            // "the tool failed".
            return Ok(ExitCode::from(2));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_diff(args: &[String]) -> Result<ExitCode, String> {
    let flags = parse_flags(args, &["--data", "--loadouts"])?;
    let [before_build, after_build] = flags.positional.as_slice() else {
        let available = assay_data::versions(&flags.data)
            .map(|v| v.join(", "))
            .unwrap_or_default();
        return Err(format!(
            "diff needs two build ids; available: {}",
            if available.is_empty() {
                "none".to_string()
            } else {
                available
            }
        ));
    };
    let before = assay_data::load(&flags.data, before_build).map_err(|e| e.to_string())?;
    let after = assay_data::load(&flags.data, after_build).map_err(|e| e.to_string())?;

    println!(
        "{} ({}) -> {} ({})",
        before.manifest.label, before_build, after.manifest.label, after_build
    );

    let changes = assay_diff::dataset_diff(&before, &after);
    if changes.is_empty() {
        println!(
            "
  no data changes"
        );
    } else {
        println!(
            "
  data ({} change(s)):",
            changes.len()
        );
        for change in &changes {
            println!("  {change}");
        }
    }

    if let Some(dir) = &flags.loadouts {
        let loadouts = read_loadouts(dir)?;
        if loadouts.is_empty() {
            return Err(format!("no .toml loadouts in {}", dir.display()));
        }
        println!(
            "
  impact on {} loadout(s):",
            loadouts.len()
        );
        for impact in assay_diff::impact_diff(&before, &after, &loadouts) {
            println!("    {}", impact.name);
            if let Some(error) = &impact.error {
                println!("      ! {error}");
                continue;
            }
            let moved: Vec<_> = impact.stats.iter().filter(|s| s.changed()).collect();
            if moved.is_empty() {
                println!("      unchanged");
            }
            for stat in moved {
                let label = stat.id.strip_prefix("derived.").unwrap_or(&stat.id);
                println!(
                    "      {label:<22} {} -> {}  ({:+})",
                    stat.from,
                    stat.to,
                    stat.delta()
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn read_loadouts(dir: &Path) -> Result<Vec<assay_core::Loadout>, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    // Sorted so the report is stable run to run.
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            loadout_file::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
        })
        .collect()
}

fn cmd_exchange(args: &[String]) -> Result<ExitCode, String> {
    let flags = parse_flags(args, &["--data", "--build", "--explain"])?;
    let [attacker_path, defender_path] = flags.positional.as_slice() else {
        return Err("exchange needs an attacker and a defender loadout".to_string());
    };

    let build = match &flags.build {
        Some(build) => build.clone(),
        None => newest_build(&flags.data)?,
    };
    let dataset = assay_data::load(&flags.data, &build).map_err(|e| e.to_string())?;

    let attacker_loadout = read_loadout(attacker_path)?;
    let defender_loadout = read_loadout(defender_path)?;
    let attacker = resolve(&attacker_loadout, &dataset.entities).map_err(|e| e.to_string())?;
    let defender = resolve(&defender_loadout, &dataset.entities).map_err(|e| e.to_string())?;

    // A weapon is required: a "basic attack" with no weapon has no base
    // damage, and inventing one would be the tool guessing.
    let weapon_id = attacker_loadout.weapons.main_hand.as_ref().ok_or_else(|| {
        format!(
            "{} has no [weapons] main_hand, so there is nothing to swing",
            attacker_loadout.name
        )
    })?;
    let item = dataset
        .entities
        .item(weapon_id)
        .ok_or_else(|| format!("item not in dataset: {weapon_id}"))?;
    let profile = item
        .weapon
        .as_ref()
        .ok_or_else(|| format!("{} is not a weapon", item.name))?;

    let strike = Strike::basic_swing(profile);
    let context = ExchangeContext::default();
    let outcome = Exchange::new(&attacker, &defender, &strike, &context, &dataset.entities)
        .damage()
        .map_err(|e| e.to_string())?;

    println!(
        "{} swinging {} at {}   {} ({build})",
        attacker_loadout.name, item.name, defender_loadout.name, dataset.manifest.label
    );
    println!();
    println!(
        "  {} damage                {:>12}",
        marker(outcome.damage.level()),
        outcome.damage.value().value()
    );
    println!(
        "  {} defender effective PDR {:>11}",
        marker(outcome.effective_pdr.level()),
        outcome.effective_pdr.value().value()
    );
    if let Some(note) = outcome.damage.note() {
        println!(
            "
  assumptions:
    {note}"
        );
    }
    if flags.explain {
        println!(
            "
  exchange (ADR-006):"
        );
        for step in &outcome.trace {
            println!("    {}. {:<24} {}", step.stage, step.label, step.detail);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn read_loadout(path: &str) -> Result<assay_core::Loadout, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    loadout_file::parse(&text).map_err(|e| format!("{path}: {e}"))
}

/// The newest committed build, by the sort order `versions` returns.
fn newest_build(data: &Path) -> Result<String, String> {
    let versions = assay_data::versions(data).map_err(|e| e.to_string())?;
    versions
        .last()
        .cloned()
        .ok_or_else(|| format!("no dataset versions under {}", data.display()))
}

fn marker(level: ConfidenceLevel) -> &'static str {
    match level {
        ConfidenceLevel::Verified => "ok",
        ConfidenceLevel::Unverified => "~ ",
        ConfidenceLevel::Unknown => "? ",
    }
}

fn print_table(name: &str, build: &str, label: &str, resolved: &Resolved) {
    println!("{name}   {label} ({build})");
    println!();

    let attributes = &resolved.attributes;
    println!(
        "  {} attributes            {}",
        marker(attributes.level()),
        render_attributes(attributes)
    );

    for (id, value) in &resolved.derived {
        // Strip the `derived.` prefix for reading; the canonical form keeps it.
        let label = id.as_str().strip_prefix("derived.").unwrap_or(id.as_str());
        println!(
            "  {} {label:<21} {:>12}",
            marker(value.level()),
            value.value()
        );
    }

    let notes: Vec<(&str, &str)> = resolved
        .derived
        .iter()
        .filter_map(|(id, value)| value.note().map(|note| (id.as_str(), note)))
        .collect();
    if !notes.is_empty() {
        println!("\n  assumptions:");
        for (id, note) in notes {
            println!("    {id}: {note}");
        }
    }

    let unverified = resolved
        .derived
        .values()
        .filter(|v| v.level() != ConfidenceLevel::Verified)
        .count();
    if unverified > 0 {
        println!(
            "\n  {unverified} of {} values are not verified against the game.",
            resolved.derived.len()
        );
    }
}

fn render_attributes(attributes: &Confidence<assay_core::AttributeBlock>) -> String {
    let block = attributes.value();
    assay_core::AttributeKind::ALL
        .into_iter()
        .map(|kind| format!("{} {}", &kind.as_str()[..3], block.get(kind).points()))
        .collect::<Vec<_>>()
        .join("  ")
}

fn print_explain(resolved: &Resolved) {
    println!("\n  pipeline (ADR-005):");
    for note in &resolved.trace {
        println!("    {}. {:<28} {}", note.stage, note.label, note.detail);
    }
}
