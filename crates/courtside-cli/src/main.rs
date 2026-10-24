use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand, ValueEnum};
use courtside_ffi::{plays, stats, validate};
use dvw_codec::Dvw;

use vb_engine::validate::Severity;

/// Which positive/negative symbols the rates use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Symbols {
    /// DataVolley 4's default efficiency table
    Datavolley,
    /// VolleyStation Pro's default winning/losing symbols
    Volleystation,
}

impl From<Symbols> for stats::Symbols {
    fn from(s: Symbols) -> Self {
        match s {
            Symbols::Datavolley => stats::Symbols::Datavolley,
            Symbols::Volleystation => stats::Symbols::Volleystation,
        }
    }
}

#[derive(Parser)]
#[command(name = "courtside", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check a scouting file for errors. Exits 1 if any error is found.
    Validate {
        file: PathBuf,
        /// Also list info notes (e.g. rotations the scout corrected)
        #[arg(long)]
        info: bool,
        /// Print every issue (info included) as JSON
        #[arg(long)]
        json: bool,
    },
    /// Convert a .dvw to .json (openvolley plays layout) or re-save as .dvw
    Convert { input: PathBuf, output: PathBuf },
    /// Print set scores and per-team and per-player skill statistics (.dvw)
    Stats {
        file: PathBuf,
        /// Print JSON instead of a text summary
        #[arg(long)]
        json: bool,
        /// Which positive/negative symbols the rates use
        #[arg(long, value_enum, default_value_t = Symbols::Datavolley)]
        symbols: Symbols,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn read(path: &Path) -> anyhow::Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

fn run(command: Command) -> anyhow::Result<ExitCode> {
    match command {
        Command::Validate { file, info, json } => {
            let issues = validate::validate(&file, &read(&file)?);
            let errors = issues
                .iter()
                .filter(|i| i.severity == Severity::Error)
                .count();
            let exit = if errors > 0 {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            };
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&validate::to_json(&issues))?
                );
                return Ok(exit);
            }
            for i in issues
                .iter()
                .filter(|i| info || i.severity != Severity::Info)
            {
                println!("{}:{i}", file.display());
            }
            let count = |s: Severity| issues.iter().filter(|i| i.severity == s).count();
            let notes = count(Severity::Info);
            eprintln!(
                "{}: {errors} error(s), {} warning(s), {notes} info{}",
                file.display(),
                count(Severity::Warning),
                if info || notes == 0 {
                    ""
                } else {
                    " (--info to list)"
                }
            );
            Ok(exit)
        }
        Command::Convert { input, output } => {
            if extension(&input) != "dvw" {
                bail!("only .dvw input is supported so far");
            }
            let dvw = Dvw::parse(&read(&input)?)
                .with_context(|| format!("cannot parse {}", input.display()))?;
            let bytes = match extension(&output).as_str() {
                "json" => {
                    let mut out = serde_json::to_vec_pretty(&plays::to_json(&dvw))?;
                    out.push(b'\n');
                    out
                }
                "dvw" => dvw.to_bytes()?,
                other => bail!("cannot convert to .{other}; use .json or .dvw"),
            };
            if output.exists() && std::fs::canonicalize(&output)? == std::fs::canonicalize(&input)?
            {
                bail!("output is the same file as input");
            }
            std::fs::write(&output, bytes)
                .with_context(|| format!("cannot write {}", output.display()))?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Stats {
            file,
            json,
            symbols,
        } => {
            if extension(&file) != "dvw" {
                bail!("only .dvw input is supported so far");
            }
            let dvw = Dvw::parse(&read(&file)?)
                .with_context(|| format!("cannot parse {}", file.display()))?;
            let report = stats::report(&dvw, symbols.into());
            if json {
                println!("{}", serde_json::to_string_pretty(&report.to_json())?);
            } else {
                print!("{}", report.to_text());
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
