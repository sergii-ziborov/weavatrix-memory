use super::super::{ExtractionInput, ExtractionOutput, TextSpan};
use crate::error::{MemoryError, Result};
use std::collections::BTreeSet;

pub(super) fn validate_output(output: &ExtractionOutput, input: &ExtractionInput) -> Result<()> {
    let mut entities = BTreeSet::new();
    for entity in &output.entities {
        entity.validate()?;
        validate_span(entity.span, input)?;
        if !entities.insert(entity.local_id.as_str()) {
            return Err(MemoryError::InvalidValue {
                field: "extraction.entities",
                reason: "local identifiers must be unique",
            });
        }
    }
    let mut relations = BTreeSet::new();
    for relation in &output.relations {
        relation.validate()?;
        validate_span(relation.span, input)?;
        if !relations.insert(relation.local_id.as_str()) {
            return Err(MemoryError::InvalidValue {
                field: "extraction.relations",
                reason: "local identifiers must be unique",
            });
        }
        if !entities.contains(relation.source.as_str())
            || !entities.contains(relation.target.as_str())
        {
            return Err(MemoryError::InvalidValue {
                field: "extraction.relation.endpoint",
                reason: "must reference extracted entity local identifiers",
            });
        }
        let valid_from = relation.valid_from.unwrap_or(input.occurred_at);
        if relation
            .valid_until
            .is_some_and(|until| until <= valid_from)
        {
            return Err(MemoryError::InvalidValue {
                field: "extraction.relation.valid_until",
                reason: "must be later than valid_from",
            });
        }
    }
    Ok(())
}

fn validate_span(span: Option<TextSpan>, input: &ExtractionInput) -> Result<()> {
    if let Some(span) = span
        && (span.start >= span.end
            || span.end > input.content.len()
            || !input.content.is_char_boundary(span.start)
            || !input.content.is_char_boundary(span.end))
    {
        return Err(MemoryError::InvalidValue {
            field: "extraction.span",
            reason: "must be a valid UTF-8 byte range inside source content",
        });
    }
    Ok(())
}
