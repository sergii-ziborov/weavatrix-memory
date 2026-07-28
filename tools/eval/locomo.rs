use super::model::{Document, PreparedBenchmark, PreparedCase};
use blazingly_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub fn prepare(bytes: &[u8]) -> Result<PreparedBenchmark, String> {
    let samples =
        blazingly_json::from_slice::<Vec<Value>>(bytes).map_err(|error| error.to_string())?;
    let mut groups = BTreeMap::new();
    let mut cases = Vec::new();
    for (sample_index, sample) in samples.iter().enumerate() {
        let sample_id = sample
            .get("sample_id")
            .and_then(Value::as_str)
            .map_or_else(|| format!("sample-{sample_index}"), str::to_owned);
        let documents = conversation_documents(sample, &sample_id)?;
        let document_ids = documents
            .iter()
            .map(|document| document.id.as_str())
            .collect::<BTreeSet<_>>();
        let qa = sample
            .get("qa")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{sample_id} has no qa array"))?;
        for (question_index, question) in qa.iter().enumerate() {
            let query = required_string(question, "question")?;
            let evidence = question
                .get("evidence")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .flat_map(split_evidence)
                .map(|id| document_id(&sample_id, id))
                .filter(|id| document_ids.contains(id.as_str()))
                .collect::<BTreeSet<_>>();
            if evidence.is_empty() {
                continue;
            }
            let category = question
                .get("category")
                .map_or_else(|| "unknown".to_owned(), value_text);
            cases.push(PreparedCase {
                id: format!("{sample_id}::qa::{question_index}"),
                group_id: sample_id.clone(),
                category: format!("locomo_{category}"),
                query,
                relevant_ids: evidence,
            });
        }
        groups.insert(sample_id, documents);
    }
    Ok(PreparedBenchmark {
        name: "locomo".to_owned(),
        groups,
        cases,
    })
}

fn conversation_documents(sample: &Value, sample_id: &str) -> Result<Vec<Document>, String> {
    let conversation = sample
        .get("conversation")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{sample_id} has no conversation object"))?;
    let mut documents = Vec::new();
    for (key, turns) in conversation {
        if !key.starts_with("session_") || key.ends_with("_date_time") {
            continue;
        }
        let Some(turns) = turns.as_array() else {
            continue;
        };
        for turn in turns {
            let Some(id) = turn.get("dia_id").and_then(Value::as_str) else {
                continue;
            };
            let Some(text) = turn.get("text").and_then(Value::as_str) else {
                continue;
            };
            let speaker = turn
                .get("speaker")
                .and_then(Value::as_str)
                .unwrap_or("speaker");
            documents.push(Document {
                id: document_id(sample_id, id),
                text: format!("{speaker}: {text}"),
            });
        }
    }
    documents.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(documents)
}

fn split_evidence(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn document_id(sample: &str, dialog: &str) -> String {
    format!("{sample}::dialog::{dialog}")
}

fn required_string(value: &Value, field: &str) -> Result<String, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing string field {field}"))
}

fn value_text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

#[cfg(test)]
mod tests {
    #[test]
    fn adapter_splits_compound_dialog_evidence() {
        let input = br#"[
          {
            "sample_id":"conv",
            "qa":[
              {"question":"What happened?","category":2,"evidence":["D1:1; D1:2"]},
              {"question":"Unknown?","category":3,"evidence":[]}
            ],
            "conversation":{
              "speaker_a":"A",
              "speaker_b":"B",
              "session_1_date_time":"today",
              "session_1":[
                {"speaker":"A","dia_id":"D1:1","text":"first"},
                {"speaker":"B","dia_id":"D1:2","text":"second"}
              ]
            }
          }
        ]"#;
        let benchmark = super::prepare(input).unwrap();

        assert_eq!(benchmark.groups["conv"].len(), 2);
        assert_eq!(benchmark.cases.len(), 1);
        assert_eq!(benchmark.cases[0].relevant_ids.len(), 2);
    }
}
