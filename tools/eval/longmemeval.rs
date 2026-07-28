use super::model::{Document, PreparedBenchmark, PreparedCase};
use blazingly_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub fn prepare(bytes: &[u8]) -> Result<PreparedBenchmark, String> {
    let samples =
        blazingly_json::from_slice::<Vec<Value>>(bytes).map_err(|error| error.to_string())?;
    let mut groups = BTreeMap::new();
    let mut cases = Vec::new();
    for sample in samples {
        let id = required_string(&sample, "question_id")?;
        if id.ends_with("_abs") {
            continue;
        }
        let sessions = required_array(&sample, "haystack_sessions")?;
        let session_ids = required_array(&sample, "haystack_session_ids")?;
        if sessions.len() != session_ids.len() {
            return Err(format!("{id} session arrays have different lengths"));
        }
        let documents = sessions
            .iter()
            .zip(session_ids)
            .map(|(session, session_id)| {
                let session_id = value_text(session_id);
                Ok(Document {
                    id: document_id(&id, &session_id),
                    text: session_text(session)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let document_ids = documents
            .iter()
            .map(|document| document.id.as_str())
            .collect::<BTreeSet<_>>();
        let relevant_ids = required_array(&sample, "answer_session_ids")?
            .iter()
            .map(value_text)
            .map(|session| document_id(&id, &session))
            .filter(|session| document_ids.contains(session.as_str()))
            .collect::<BTreeSet<_>>();
        if !relevant_ids.is_empty() {
            cases.push(PreparedCase {
                id: id.clone(),
                group_id: id.clone(),
                category: required_string(&sample, "question_type")
                    .unwrap_or_else(|_| "unknown".to_owned()),
                query: required_string(&sample, "question")?,
                relevant_ids,
            });
        }
        groups.insert(id, documents);
    }
    Ok(PreparedBenchmark {
        name: "longmemeval".to_owned(),
        groups,
        cases,
    })
}

fn session_text(session: &Value) -> Result<String, String> {
    let turns = session
        .as_array()
        .ok_or_else(|| "session is not an array".to_owned())?;
    let mut text = String::new();
    for turn in turns {
        let role = turn.get("role").and_then(Value::as_str).unwrap_or("turn");
        let content = turn
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| "turn content is not a string".to_owned())?;
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(role);
        text.push_str(": ");
        text.push_str(content);
    }
    Ok(text)
}

fn document_id(question: &str, session: &str) -> String {
    format!("{question}::session::{session}")
}

fn required_string(value: &Value, field: &str) -> Result<String, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing string field {field}"))
}

fn required_array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("missing array field {field}"))
}

fn value_text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

#[cfg(test)]
mod tests {
    #[test]
    fn adapter_skips_official_abstention_cases() {
        let input = br#"[
          {
            "question_id":"keep",
            "question_type":"knowledge-update",
            "question":"What changed?",
            "haystack_session_ids":["s1"],
            "haystack_sessions":[[{"role":"user","content":"new value"}]],
            "answer_session_ids":["s1"]
          },
          {
            "question_id":"skip_abs",
            "question_type":"abstention",
            "question":"Unknown?",
            "haystack_session_ids":["s2"],
            "haystack_sessions":[[{"role":"user","content":"filler"}]],
            "answer_session_ids":["s2"]
          }
        ]"#;
        let benchmark = super::prepare(input).unwrap();

        assert_eq!(benchmark.cases.len(), 1);
        assert_eq!(benchmark.groups.len(), 1);
        assert_eq!(benchmark.cases[0].id, "keep");
    }
}
