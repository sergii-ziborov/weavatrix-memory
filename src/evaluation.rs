use crate::error::{MemoryError, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationCase {
    pub id: String,
    pub category: String,
    pub relevant_ids: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankedPrediction {
    pub case_id: String,
    pub ranked_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalMetrics {
    pub cases: usize,
    pub hit_at: BTreeMap<usize, f64>,
    pub recall_at: BTreeMap<usize, f64>,
    pub ndcg_at: BTreeMap<usize, f64>,
    pub mean_reciprocal_rank: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub overall: RetrievalMetrics,
    pub by_category: BTreeMap<String, RetrievalMetrics>,
}

/// Scores ranked evidence retrieval without an LLM judge.
///
/// The same metrics are usable for `LoCoMo` dialog evidence, `LongMemEval`
/// session evidence, and coding-agent provenance. Missing predictions count as
/// misses; unknown and duplicate case identifiers are rejected.
///
/// # Errors
///
/// Rejects empty cutoffs, duplicate identifiers, empty relevance sets, and
/// predictions for unknown cases.
pub fn evaluate_retrieval(
    cases: &[EvaluationCase],
    predictions: &[RankedPrediction],
    cutoffs: &[usize],
) -> Result<EvaluationReport> {
    validate_cutoffs(cutoffs)?;
    let cases_by_id = validate_cases(cases)?;
    let predictions = validate_predictions(predictions, &cases_by_id)?;
    let overall = score(cases.iter(), &predictions, cutoffs);
    let mut categories = BTreeMap::<String, Vec<&EvaluationCase>>::new();
    for case in cases {
        categories
            .entry(case.category.clone())
            .or_default()
            .push(case);
    }
    let by_category = categories
        .into_iter()
        .map(|(category, cases)| (category, score(cases.into_iter(), &predictions, cutoffs)))
        .collect();
    Ok(EvaluationReport {
        overall,
        by_category,
    })
}

fn validate_cutoffs(cutoffs: &[usize]) -> Result<()> {
    if cutoffs.is_empty() || cutoffs.contains(&0) {
        return Err(invalid("cutoffs must be non-empty and greater than zero"));
    }
    Ok(())
}

fn validate_cases(cases: &[EvaluationCase]) -> Result<BTreeMap<&str, &EvaluationCase>> {
    let mut indexed = BTreeMap::new();
    for case in cases {
        if case.id.is_empty()
            || case.category.is_empty()
            || case.relevant_ids.is_empty()
            || indexed.insert(case.id.as_str(), case).is_some()
        {
            return Err(invalid(
                "cases need unique non-empty ids, categories, and relevance",
            ));
        }
    }
    Ok(indexed)
}

fn validate_predictions<'a>(
    predictions: &'a [RankedPrediction],
    cases: &BTreeMap<&str, &EvaluationCase>,
) -> Result<BTreeMap<&'a str, Vec<&'a str>>> {
    let mut indexed = BTreeMap::new();
    for prediction in predictions {
        if !cases.contains_key(prediction.case_id.as_str()) {
            return Err(invalid("prediction references an unknown case"));
        }
        let mut seen = BTreeSet::new();
        let ranked = prediction
            .ranked_ids
            .iter()
            .map(String::as_str)
            .filter(|id| seen.insert(*id))
            .collect::<Vec<_>>();
        if indexed
            .insert(prediction.case_id.as_str(), ranked)
            .is_some()
        {
            return Err(invalid("prediction case ids must be unique"));
        }
    }
    Ok(indexed)
}

fn score<'a>(
    cases: impl Iterator<Item = &'a EvaluationCase>,
    predictions: &BTreeMap<&str, Vec<&str>>,
    cutoffs: &[usize],
) -> RetrievalMetrics {
    let cases = cases.collect::<Vec<_>>();
    let mut hit_at = cutoffs
        .iter()
        .map(|cutoff| (*cutoff, 0.0))
        .collect::<BTreeMap<_, _>>();
    let mut recall_at = hit_at.clone();
    let mut ndcg_at = hit_at.clone();
    let mut reciprocal_rank = 0.0;
    for case in &cases {
        let ranked = predictions
            .get(case.id.as_str())
            .map_or(&[][..], Vec::as_slice);
        reciprocal_rank += ranked
            .iter()
            .position(|id| case.relevant_ids.contains(*id))
            .map_or(0.0, |index| 1.0 / float(index + 1));
        for cutoff in cutoffs {
            let selected = &ranked[..ranked.len().min(*cutoff)];
            let relevant = selected
                .iter()
                .filter(|id| case.relevant_ids.contains(**id))
                .count();
            hit_at.entry(*cutoff).and_modify(|value| {
                *value += f64::from(relevant > 0);
            });
            recall_at.entry(*cutoff).and_modify(|value| {
                *value += float(relevant) / float(case.relevant_ids.len());
            });
            ndcg_at
                .entry(*cutoff)
                .and_modify(|value| *value += ndcg(selected, &case.relevant_ids, *cutoff));
        }
    }
    let denominator = float(cases.len().max(1));
    for metrics in [&mut hit_at, &mut recall_at, &mut ndcg_at] {
        for value in metrics.values_mut() {
            *value /= denominator;
        }
    }
    RetrievalMetrics {
        cases: cases.len(),
        hit_at,
        recall_at,
        ndcg_at,
        mean_reciprocal_rank: reciprocal_rank / denominator,
    }
}

fn ndcg(selected: &[&str], relevant: &BTreeSet<String>, cutoff: usize) -> f64 {
    let dcg = selected
        .iter()
        .enumerate()
        .filter(|(_, id)| relevant.contains(**id))
        .map(|(index, _)| 1.0 / float(index + 2).log2())
        .sum::<f64>();
    let ideal = (0..relevant.len().min(cutoff))
        .map(|index| 1.0 / float(index + 2).log2())
        .sum::<f64>();
    if ideal == 0.0 { 0.0 } else { dcg / ideal }
}

fn invalid(reason: &'static str) -> MemoryError {
    MemoryError::InvalidValue {
        field: "evaluation",
        reason,
    }
}

fn float(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}
