mod locomo;
mod longmemeval;
mod model;

use model::PreparedBenchmark;
use std::{env, error::Error, fs, path::Path};
use weavatrix_memory::{RankedPrediction, evaluate_retrieval};

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [command, input, output] if command == "prepare-locomo" => {
            let benchmark = locomo::prepare(&fs::read(input)?)?;
            write_prepared(&benchmark, output)?;
        }
        [command, input, output] if command == "prepare-longmemeval" => {
            let benchmark = longmemeval::prepare(&fs::read(input)?)?;
            write_prepared(&benchmark, output)?;
        }
        [command, input, output] if command == "validate-coding" => {
            let benchmark = read_prepared(input)?;
            benchmark.validate()?;
            write_prepared(&benchmark, output)?;
        }
        [command, input, output, limit] if command == "literal" => {
            let benchmark = read_prepared(input)?;
            benchmark.validate()?;
            let limit = limit.parse::<usize>()?;
            let predictions = benchmark.literal_predictions(limit);
            fs::write(output, blazingly_json::to_vec_pretty(&predictions)?)?;
        }
        [command, benchmark, predictions, output] if command == "score" => {
            let benchmark = read_prepared(benchmark)?;
            benchmark.validate()?;
            let predictions = read_predictions(predictions)?;
            let report =
                evaluate_retrieval(&benchmark.evaluation_cases(), &predictions, &[1, 5, 10])?;
            fs::write(output, blazingly_json::to_vec_pretty(&report)?)?;
            println!(
                "cases={} hit@5={:.4} recall@5={:.4} mrr={:.4}",
                report.overall.cases,
                report.overall.hit_at[&5],
                report.overall.recall_at[&5],
                report.overall.mean_reciprocal_rank
            );
        }
        _ => print_usage(),
    }
    Ok(())
}

fn read_prepared(path: impl AsRef<Path>) -> Result<PreparedBenchmark, Box<dyn Error>> {
    Ok(blazingly_json::from_slice(&fs::read(path)?)?)
}

fn write_prepared(
    benchmark: &PreparedBenchmark,
    path: impl AsRef<Path>,
) -> Result<(), Box<dyn Error>> {
    benchmark.validate()?;
    fs::write(path, blazingly_json::to_vec(benchmark)?)?;
    Ok(())
}

fn read_predictions(path: impl AsRef<Path>) -> Result<Vec<RankedPrediction>, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    if let Ok(predictions) = blazingly_json::from_slice(&bytes) {
        return Ok(predictions);
    }
    let text = std::str::from_utf8(&bytes)?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(blazingly_json::from_str)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn print_usage() {
    eprintln!(
        "usage:\n  weavatrix-memory-eval prepare-locomo INPUT OUTPUT\n  \
         weavatrix-memory-eval prepare-longmemeval INPUT OUTPUT\n  \
         weavatrix-memory-eval validate-coding INPUT OUTPUT\n  \
         weavatrix-memory-eval literal PREPARED PREDICTIONS LIMIT\n  \
         weavatrix-memory-eval score PREPARED PREDICTIONS REPORT"
    );
}
