use super::{
    ContextBundle, ContextCompiler, ContextRequest, FusedRetrievalHit, RetrievalProvider,
    RetrievalQuery, TokenEstimator, fuse_retrieval,
};
use crate::{MemoryError, MemoryProjection, Result};

#[derive(Debug, Clone)]
pub struct RetrievedContextBundle {
    pub context: ContextBundle,
    pub retrieval: Vec<FusedRetrievalHit>,
}

impl<T> ContextCompiler<T>
where
    T: TokenEstimator,
{
    /// Resolves free text into exact graph seeds and compiles their context.
    ///
    /// Providers can be lexical, BM25, vector, or hybrid implementations from
    /// separate crates. Rank fusion is deterministic and provider scores never
    /// need to share a scale.
    ///
    /// # Errors
    ///
    /// Returns provider, missing-entity, scope, graph, or budget failures.
    pub fn compile_with_retrieval(
        &self,
        projection: &MemoryProjection,
        request: &ContextRequest,
        query: &RetrievalQuery,
        providers: &[&dyn RetrievalProvider],
    ) -> Result<RetrievedContextBundle> {
        let retrieval = fuse_retrieval(providers, query).map_err(MemoryError::from)?;
        let seeds = retrieval
            .iter()
            .filter(|hit| {
                projection
                    .visible_node(&hit.entity, request.known_at)
                    .is_some()
            })
            .map(|hit| hit.entity.clone())
            .collect::<Vec<_>>();
        if seeds.is_empty() {
            return Err(MemoryError::Retrieval {
                provider: "fusion".to_owned(),
                message: "no retrieved entity exists in this projection".to_owned(),
            });
        }
        let mut exact = request.clone();
        exact.seeds = seeds;
        Ok(RetrievedContextBundle {
            context: self.compile(projection, &exact)?,
            retrieval,
        })
    }
}
