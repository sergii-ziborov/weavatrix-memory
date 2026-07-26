mod common;

use common::{entity, simple_projection, ts};
use weavatrix_memory::{
    ContextCompiler, ContextRequest, MemoryError, RetrievalChannel, RetrievalError, RetrievalHit,
    RetrievalProvider, RetrievalQuery, RetrievalResult, fuse_retrieval,
};

struct Provider {
    name: &'static str,
    hits: Vec<RetrievalHit>,
    fail: bool,
}

impl RetrievalProvider for Provider {
    fn name(&self) -> &str {
        self.name
    }

    fn retrieve(&self, _query: &RetrievalQuery) -> RetrievalResult<Vec<RetrievalHit>> {
        if self.fail {
            Err(RetrievalError::new(self.name, "offline"))
        } else {
            Ok(self.hits.clone())
        }
    }
}

fn provider(name: &'static str, hits: Vec<RetrievalHit>) -> Provider {
    Provider {
        name,
        hits,
        fail: false,
    }
}

#[test]
fn reciprocal_rank_fusion_is_deterministic_and_scale_independent() {
    let lexical = provider(
        "lexical",
        vec![
            RetrievalHit::new(entity("task:1"), RetrievalChannel::Lexical, 900),
            RetrievalHit::new(entity("file:1"), RetrievalChannel::Lexical, 800),
            RetrievalHit::new(entity("task:1"), RetrievalChannel::Literal, 700),
        ],
    );
    let semantic = provider(
        "semantic",
        vec![
            RetrievalHit::new(entity("file:1"), RetrievalChannel::Semantic, 9),
            RetrievalHit::new(entity("task:1"), RetrievalChannel::Semantic, 8),
        ],
    );
    let query = RetrievalQuery::new("one-day accuracy", 5).unwrap();

    let forward = fuse_retrieval(&[&lexical, &semantic], &query).unwrap();
    let reverse = fuse_retrieval(&[&semantic, &lexical], &query).unwrap();

    assert_eq!(forward, reverse);
    assert_eq!(forward.len(), 2);
    assert_eq!(forward[0].entity, entity("file:1"));
    assert_eq!(forward[1].entity, entity("task:1"));
    assert_eq!(forward[1].sources.len(), 2);
}

#[test]
fn channels_provider_identity_and_failures_are_checked() {
    let lexical = provider(
        "same",
        vec![
            RetrievalHit::new(entity("task:1"), RetrievalChannel::Lexical, 10),
            RetrievalHit::new(entity("file:1"), RetrievalChannel::Semantic, 100),
        ],
    );
    let duplicate = provider("same", Vec::new());
    let query = RetrievalQuery::new("task", 2)
        .unwrap()
        .include(RetrievalChannel::Lexical);

    let filtered = fuse_retrieval(&[&lexical], &query).unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].entity, entity("task:1"));
    assert!(fuse_retrieval(&[&lexical, &duplicate], &query).is_err());

    let failed = Provider {
        name: "vector",
        hits: Vec::new(),
        fail: true,
    };
    assert!(fuse_retrieval(&[&failed], &query).is_err());
}

#[test]
fn retrieval_seeds_the_exact_evidence_graph() {
    let projection = simple_projection();
    let lexical = provider(
        "weavatrix-search",
        vec![
            RetrievalHit::new(entity("missing:1"), RetrievalChannel::Lexical, 100),
            RetrievalHit::new(entity("task:1"), RetrievalChannel::Lexical, 90),
        ],
    );
    let request = ContextRequest::for_retrieval(ts(10), ts(10), 10_000).unwrap();
    let query = RetrievalQuery::new("query accuracy", 5).unwrap();

    let bundle = ContextCompiler::default()
        .compile_with_retrieval(&projection, &request, &query, &[&lexical])
        .unwrap();

    assert_eq!(bundle.context.graph.node_count(), 2);
    assert_eq!(bundle.context.graph.edge_count(), 1);
    assert_eq!(bundle.retrieval.len(), 2);
    assert!(matches!(
        ContextCompiler::default().compile(&projection, &request),
        Err(MemoryError::InvalidValue {
            field: "context.seeds",
            ..
        })
    ));
}
