mod common;

use common::{agent, entity, node, session, ts};
use weavatrix_memory::{
    AutoExtractionEngine, Confidence, EntityLinker, ExtractedEntity, ExtractionError,
    ExtractionInput, ExtractionOutput, ExtractionProvider, LinkMethod, LinkPolicy, MemoryEvent,
    MemoryView, TextSpan,
};

#[derive(Clone)]
struct Provider(ExtractionOutput);

impl ExtractionProvider for Provider {
    fn name(&self) -> &'static str {
        "linking-test"
    }

    fn extract(&self, _input: &ExtractionInput) -> Result<ExtractionOutput, ExtractionError> {
        Ok(self.0.clone())
    }
}

fn input() -> ExtractionInput {
    ExtractionInput::new(
        "message:link",
        "short alias",
        ts(1),
        ts(1),
        agent(),
        session(),
    )
    .unwrap()
}

#[test]
fn mention_label_matches_catalog_alias() {
    let linker = EntityLinker::from_view(&MemoryView {
        nodes: vec![
            node("fn:canonical", "function", "Canonical Name")
                .with_attribute("alias.0", "short-alias"),
        ],
        facts: Vec::new(),
    })
    .unwrap();
    let mention =
        ExtractedEntity::new("mention", "function", "Short Alias", Confidence::CERTAIN).unwrap();

    let link = linker
        .link(&mention, &input(), LinkPolicy::default())
        .unwrap();

    assert_eq!(link.method, LinkMethod::Alias);
    assert_eq!(link.entity_id.unwrap().as_str(), "fn:canonical");
}

#[test]
fn repeated_new_entity_mentions_are_merged_deterministically() {
    let first = ExtractedEntity::new("first", "task", "Same task", Confidence::CERTAIN)
        .unwrap()
        .with_alias("ONE");
    let second = ExtractedEntity::new("second", "task", "Same task", Confidence::CERTAIN)
        .unwrap()
        .with_alias("TWO");
    let provider = Provider(ExtractionOutput {
        entities: vec![first, second],
        relations: Vec::new(),
    });

    let plan = AutoExtractionEngine::default()
        .plan(&provider, &input(), &MemoryView::default())
        .unwrap();

    assert_eq!(plan.node_event_count(), 1);
    assert_eq!(plan.links[0].entity_id, plan.links[1].entity_id);
    let MemoryEvent::NodeUpserted { node } = &plan.events[0].payload else {
        panic!("expected node event");
    };
    let aliases = node
        .attributes
        .iter()
        .filter(|(key, _)| key.starts_with("alias."))
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>();
    assert!(aliases.contains(&"ONE"));
    assert!(aliases.contains(&"TWO"));
}

#[test]
fn malformed_catalog_and_deserialized_span_are_rejected() {
    let duplicate = node("entity:duplicate", "file", "same");
    assert!(
        EntityLinker::from_view(&MemoryView {
            nodes: vec![duplicate.clone(), duplicate],
            facts: Vec::new(),
        })
        .is_err()
    );

    let mention = ExtractedEntity::new("mention", "file", "same", Confidence::CERTAIN)
        .unwrap()
        .with_span(TextSpan { start: 5, end: 2 })
        .with_stable_id(entity("entity:duplicate"));
    let provider = Provider(ExtractionOutput {
        entities: vec![mention],
        relations: Vec::new(),
    });
    assert!(
        AutoExtractionEngine::default()
            .plan(&provider, &input(), &MemoryView::default())
            .is_err()
    );
}
