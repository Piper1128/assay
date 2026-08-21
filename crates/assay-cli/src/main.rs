//! The `assay` binary — the only crate that may write to stdout
//! (ADR-000 rev 2).
//!
//! Subcommands exist when they work. `diff` needs two dataset versions to
//! say anything; with only one committed it reports that plainly rather than
//! printing an empty result that reads like "nothing changed".

mod loadout_file;
mod situation;
mod submit;

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
    assay exchange <attacker.toml> <defender.toml> [--situation <s.toml>] [OPTIONS]
    assay diff <build-a> <build-b> [--loadouts <dir>] [--data <dir>]
    assay versions [--data <dir>]
    assay submit <submission.json> [--apply] [--data <dir>] [--build <id>]

RESOLVE OPTIONS:
    --build <id>     dataset build to resolve against (default: the newest
                     committed build)
    --data <dir>     dataset root (default: ./data)
    --explain        print every pipeline stage: input, transformation, output
    --json           print the canonical form instead of a table
    --strict         exit 2 if any value is below `verified` (ADR-007)

EXCHANGE OPTIONS:
    --situation <f>  what makes this attack different from a plain swing:
                     the skill's scaling and bonuses, and the circumstances
                     around it. Omitted, the weapon simply swings.
    --explain        print all nine damage steps (ADR-006)
    --build / --data as for resolve

SUBMIT OPTIONS:
    --apply          write the submission in. Without it, submit only says
                     what it would change: reading a submission is not
                     applying one, and a person stands between the two.
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
        Some("submit") => submit::cmd_submit(&args[1..]),
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
    situation: Option<PathBuf>,
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
        situation: None,
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
            "--situation" => {
                flags.situation = Some(PathBuf::from(needs_value("--situation")?));
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
    let flags = parse_flags(args, &["--data", "--build", "--explain", "--situation"])?;
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

    // A weapon is not required. A swing needs one — a basic attack with
    // nothing in hand has no base damage, and inventing one would be the
    // tool guessing at the number it exists to look up — but a spell carries
    // its own, so a caster is a complete strike with empty hands.
    let held = match attacker_loadout.weapons.main_hand.as_ref() {
        Some(weapon_id) => {
            let item = dataset
                .entities
                .item(weapon_id)
                .ok_or_else(|| format!("item not in dataset: {weapon_id}"))?;
            let profile = item
                .weapon
                .as_ref()
                .ok_or_else(|| format!("{} is not a weapon", item.name))?;
            Some((weapon_id, profile))
        }
        None => None,
    };

    // A situation is a fact about this attack, not about either character,
    // which is why it arrives as its own file. Without one the answer is an
    // unmodified swing — the only question this command could ask until now.
    let (strike, context, situation_name) = match &flags.situation {
        Some(path) => {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let parsed = situation::parse(&text, held, &dataset.entities)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            (parsed.strike, parsed.context, parsed.name)
        }
        None => {
            let (weapon_id, profile) = held.ok_or_else(|| {
                format!(
                    "{} has no [weapons] main_hand and no situation naming a \
                     skill, so there is nothing to swing and nothing to cast",
                    attacker_loadout.name
                )
            })?;
            (
                Strike::basic_swing(weapon_id, profile),
                ExchangeContext::default(),
                "an unmodified swing".to_string(),
            )
        }
    };
    let outcome = Exchange::new(&attacker, &defender, &strike, &context, &dataset.entities)
        .damage()
        .map_err(|e| e.to_string())?;

    // "swinging a mace" or "casting at", because a caster holds nothing and
    // a header that said otherwise would be describing a different fight.
    let doing = match held {
        Some((id, _)) => {
            let name = dataset
                .entities
                .item(id)
                .map_or_else(|| id.as_str().to_string(), |i| i.name.clone());
            format!("swinging {name} at")
        }
        None => "casting at".to_string(),
    };
    println!(
        "{} {doing} {}   {} ({build})",
        attacker_loadout.name, defender_loadout.name, dataset.manifest.label
    );
    // Which attack this was. Two runs that differ only by a situation file
    // would otherwise print identical headers and different numbers.
    println!("  {situation_name}");
    println!();
    println!(
        "  {} damage                {:>12}",
        marker(outcome.damage.level()),
        outcome.damage.value().value()
    );
    // What a fight is, rather than what one hit is. A reader given 27.374
    // and 109 is being asked to do arithmetic the tool exists to do.
    match outcome.hits_to_kill {
        Some(hits) => match &outcome.time_to_kill {
            Some(t) => println!(
                "     hits to kill            {hits:>12}   in {}s",
                t.value()
            ),
            None => println!(
                "     hits to kill            {hits:>12}   (no swing time measured for this weapon)"
            ),
        },
        None => {
            println!("     hits to kill                  never   this attack takes nothing off")
        }
    }
    // And what the weapon actually does, when it does more than one thing.
    // A chained weapon never lands the same blow twice running, so the line
    // above is arithmetic about a fight nobody has.
    if let Some(swings) = outcome.chain_to_kill {
        match &outcome.chain_time_to_kill {
            Some(t) => println!(
                "     swings to kill          {swings:>12}   in {}s, running the chain",
                t.value()
            ),
            None => println!("     swings to kill          {swings:>12}   running the chain"),
        }
    }
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
pub(crate) fn newest_build(data: &Path) -> Result<String, String> {
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

    // Strip the `derived.` prefix for reading; the canonical form keeps it.
    let label_of = |id: &assay_core::DerivedStatId| -> String {
        id.as_str()
            .strip_prefix("derived.")
            .unwrap_or(id.as_str())
            .to_string()
    };
    // The column is measured, not chosen. A dataset may define any stat it
    // likes (ADR-012), so a width picked for the stats that happened to
    // exist when this was written is wrong the first time someone adds one —
    // `magical_damage_reduction` is longer than every stat that came before.
    let width = resolved
        .derived
        .keys()
        .map(|id| label_of(id).len())
        .chain(core::iter::once("attributes".len()))
        .max()
        .unwrap_or_default();

    let attributes = &resolved.attributes;
    println!(
        "  {} {:<width$} {}",
        marker(attributes.level()),
        "attributes",
        render_attributes(attributes)
    );

    for (id, value) in &resolved.derived {
        println!(
            "  {} {:<width$} {:>12}",
            marker(value.level()),
            label_of(id),
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
        // Precisely: the INPUTS are unconfirmed. Several of these have been
        // checked against a character sheet at one attribute value, which does
        // not verify a curve — but saying "not verified against the game" of a
        // number that was is the kind of small untruth this tool cannot afford.
        println!(
            "\n  {unverified} of {} rest on data nobody has confirmed.",
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
    print_breakdown(resolved);
}

/// Prints each derived stat the way the game's own Details view does:
/// the total, then what the rating contributed and what was added flat.
///
/// This is the section that makes a number checkable rather than merely
/// comparable. Reading `1.5` next to the game's `1.5` says the totals agree;
/// reading `from magic_resistance 15 (1.5)` next to the game's
/// `From Magic Resistance 15 (1.5%)` says they agree *for the same reason*,
/// which is the only kind of agreement worth having when both halves came
/// from a wiki.
fn print_breakdown(resolved: &Resolved) {
    if resolved.breakdown.is_empty() {
        return;
    }
    println!("\n  where each number came from:");
    for (id, parts) in &resolved.breakdown {
        let label = id.as_str().strip_prefix("derived.").unwrap_or(id.as_str());
        let total = resolved
            .derived
            .get(id)
            .map_or_else(|| "—".to_string(), |v| v.value().to_string());
        println!("    {label:<26} {total:>12}");
        println!(
            "      from rating {:<12} {:>12}",
            parts.rating.value(),
            parts.from_rating.value()
        );
        println!("      from bonuses {:>24}", parts.from_bonuses.value());
        // The parts are stage 4's. Stages 5 and 6 still adjust move speed
        // afterwards, and a clamp may have bound, so a difference here is
        // not necessarily either one — saying which would be guessing, and
        // the stage trace above already says what happened.
        let sum = *parts.from_rating.value() + *parts.from_bonuses.value();
        if let Some(value) = resolved.derived.get(id)
            && sum != *value.value()
        {
            println!("      later stages moved it from {sum:>11}");
        }
    }
}
