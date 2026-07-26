use super::{ChangeKind, DriftReport, DriftSnapshot, MemoryAnalytics};
use crate::{EntityId, MemoryError, MemoryProjection, Result, Timestamp};

impl MemoryAnalytics {
    /// Reconstructs a source/relation belief timeline from immutable facts.
    ///
    /// # Errors
    ///
    /// Rejects an empty or whitespace-padded relation.
    pub fn drift(
        projection: &MemoryProjection,
        source: &EntityId,
        relation: &str,
        known_at: Timestamp,
    ) -> Result<DriftReport> {
        if relation.is_empty() || relation.trim() != relation {
            return Err(MemoryError::InvalidValue {
                field: "drift.relation",
                reason: "must be non-empty without surrounding whitespace",
            });
        }
        let mut facts = projection
            .all_facts()
            .iter()
            .filter(|fact| {
                fact.source == *source && fact.relation == relation && fact.recorded_at <= known_at
            })
            .collect::<Vec<_>>();
        facts.sort_by_key(|fact| (fact.recorded_at, fact.id.clone()));
        let mut snapshots = Vec::with_capacity(facts.len());
        for (index, fact) in facts.iter().enumerate() {
            let change = index
                .checked_sub(1)
                .map_or(ChangeKind::Initial, |prior| classify(facts[prior], fact));
            snapshots.push(DriftSnapshot {
                fact: fact.id.clone(),
                target: fact.target.clone(),
                recorded_at: fact.recorded_at,
                confidence_bps: fact.confidence.basis_points(),
                change,
            });
        }
        let correction_count = snapshots
            .iter()
            .filter(|snapshot| snapshot.change == ChangeKind::Corrected)
            .count();
        let stability_bps = stability(&snapshots, correction_count);
        Ok(DriftReport {
            source: source.clone(),
            relation: relation.to_owned(),
            snapshots,
            correction_count,
            stability_bps,
            likely_to_change: correction_count >= 2 || stability_bps < 6_000,
        })
    }
}

fn classify(prior: &crate::MemoryFact, current: &crate::MemoryFact) -> ChangeKind {
    if current.supersedes.as_ref() == Some(&prior.id) || current.target != prior.target {
        ChangeKind::Corrected
    } else if current.confidence > prior.confidence {
        ChangeKind::Reinforced
    } else if current.confidence < prior.confidence {
        ChangeKind::Weakened
    } else {
        ChangeKind::Refined
    }
}

fn stability(snapshots: &[DriftSnapshot], corrections: usize) -> u16 {
    let transitions = snapshots.len().saturating_sub(1);
    if transitions == 0 {
        return 10_000;
    }
    let other = transitions.saturating_sub(corrections);
    let penalty = corrections
        .saturating_mul(6_000)
        .saturating_add(other.saturating_mul(1_000))
        .saturating_div(transitions)
        .min(10_000);
    u16::try_from(10_000_usize.saturating_sub(penalty)).unwrap_or(0)
}
