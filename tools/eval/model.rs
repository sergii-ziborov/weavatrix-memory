use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use weavatrix_memory::{EvaluationCase, RankedPrediction};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedCase {
    pub id: String,
    pub group_id: String,
    pub category: String,
    pub query: String,
    pub relevant_ids: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedBenchmark {
    pub name: String,
    pub groups: BTreeMap<String, Vec<Document>>,
    pub cases: Vec<PreparedCase>,
}

impl PreparedBenchmark {
    pub fn evaluation_cases(&self) -> Vec<EvaluationCase> {
        self.cases
            .iter()
            .map(|case| EvaluationCase {
                id: case.id.clone(),
                category: case.category.clone(),
                relevant_ids: case.relevant_ids.clone(),
            })
            .collect()
    }

    pub fn literal_predictions(&self, limit: usize) -> Vec<RankedPrediction> {
        self.cases
            .iter()
            .map(|case| {
                let query = tokens(&case.query);
                let mut ranked = self.groups[&case.group_id]
                    .iter()
                    .map(|document| {
                        let terms = tokens(&document.text);
                        let score = query.intersection(&terms).count();
                        (score, document.id.as_str())
                    })
                    .collect::<Vec<_>>();
                ranked
                    .sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(right.1)));
                RankedPrediction {
                    case_id: case.id.clone(),
                    ranked_ids: ranked
                        .into_iter()
                        .take(limit)
                        .map(|(_, id)| id.to_owned())
                        .collect(),
                }
            })
            .collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut ids = BTreeSet::new();
        for case in &self.cases {
            let Some(documents) = self.groups.get(&case.group_id) else {
                return Err(format!("case {} references a missing group", case.id));
            };
            let document_ids = documents
                .iter()
                .map(|document| document.id.as_str())
                .collect::<BTreeSet<_>>();
            if case.id.is_empty()
                || case.query.is_empty()
                || case.relevant_ids.is_empty()
                || !ids.insert(case.id.as_str())
                || !case
                    .relevant_ids
                    .iter()
                    .all(|id| document_ids.contains(id.as_str()))
            {
                return Err(format!("invalid prepared case {}", case.id));
            }
        }
        Ok(())
    }
}

fn tokens(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.len() > 1)
        .map(str::to_lowercase)
        .collect()
}
