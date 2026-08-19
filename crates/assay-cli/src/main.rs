//! The `assay` binary — the only crate that may write to stdout
//! (ADR-000 rev 2).
//!
//! Two subcommands exist because two subcommands work. `diff` lands with the
//! diff engine (ADR-008) and is deliberately absent rather than stubbed: a
//! command that prints nothing useful is worse than one that is not there.

mod loadout_file;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use assay_core::confidence::ConfidenceLevel;
use assay_core::resolve::Resolved;
use assay_core::{Confidence, canonical_statblock, resolve};

const USAGE: &str = "\
assay — headless stat resolver and patch differ for Dark and Darker

USAGE:
    assay resolve <loadout.toml> [OPTIONS]
    assay versions [--data <dir>]

RESOLVE OPTIONS:
    --build <id>     dataset build to resolve against (default: the newest
                     committed build)
    --data <dir>     dataset root (default: ./data)
    --explain        print every pipeline stage: input, transformation, output
    --json           print the canonical form instead of a table
    --strict         exit 2 if any value is below `verified` (ADR-007)

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
        Some("versions") => cmd_versions(&args[1..]),
        Some("--help" | "-h" | "help") | None => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some("diff") => {
            Err("diff is not implemented yet: it lands with the diff engine (ADR-008)".to_string())
        }
        Some(other) => Err(format!("unknown command: {other}\n\n{USAGE}")),
    }
}

/// Flags parsed off an argument list, so an unknown flag is an error rather
/// than something silently ignored.
struct Flags {
    positional: Vec<String>,
    data: PathBuf,
    build: Option<String>,
    explain: bool,
    json: bool,
    strict: bool,
}

fn parse_flags(args: &[String], allowed: &[&str]) -> Result<Flags, String> {
    let mut flags = Flags {
        positional: Vec::new(),
        data: PathBuf::from("data"),
        build: None,
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

    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let loadout = loadout_file::parse(&text).map_err(|e| format!("{path}: {e}"))?;

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
