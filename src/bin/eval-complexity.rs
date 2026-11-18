use std::env;
use std::fs;
use std::process::ExitCode;

use fuxi_dmn::requirement_complexity::{MAX_DATASET_BYTES, evaluate_tsv};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("complexity evaluation failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        return Err("usage: cargo run --quiet --bin eval-complexity -- <labeled.tsv>".into());
    };
    if args.next().is_some() {
        return Err("expected exactly one labeled TSV path".into());
    }
    let metadata =
        fs::metadata(&path).map_err(|error| format!("cannot inspect dataset {path:?}: {error}"))?;
    if metadata.len() > MAX_DATASET_BYTES as u64 {
        return Err(format!("dataset exceeds {MAX_DATASET_BYTES} bytes"));
    }
    let input = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read dataset {path:?}: {error}"))?;
    let report = evaluate_tsv(&input)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("cannot serialize report: {error}"))?
    );
    Ok(())
}
