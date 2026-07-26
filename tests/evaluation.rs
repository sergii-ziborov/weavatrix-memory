use std::collections::BTreeSet;
use weavatrix_memory::{EvaluationCase, RankedPrediction, evaluate_retrieval};

fn case(id: &str, category: &str, relevant: &[&str]) -> EvaluationCase {
    EvaluationCase {
        id: id.to_owned(),
        category: category.to_owned(),
        relevant_ids: relevant.iter().map(|id| (*id).to_owned()).collect(),
    }
}

#[test]
fn retrieval_metrics_are_exact_and_category_aware() {
    let cases = [
        case("one", "temporal", &["a", "b"]),
        case("two", "provenance", &["c"]),
    ];
    let predictions = [
        RankedPrediction {
            case_id: "one".to_owned(),
            ranked_ids: vec!["x".to_owned(), "a".to_owned(), "a".to_owned()],
        },
        RankedPrediction {
            case_id: "two".to_owned(),
            ranked_ids: vec!["c".to_owned()],
        },
    ];

    let report = evaluate_retrieval(&cases, &predictions, &[1, 2]).unwrap();

    assert_eq!(report.overall.cases, 2);
    assert!((report.overall.hit_at[&1] - 0.5).abs() < f64::EPSILON);
    assert!((report.overall.hit_at[&2] - 1.0).abs() < f64::EPSILON);
    assert!((report.overall.recall_at[&2] - 0.75).abs() < f64::EPSILON);
    assert!((report.overall.mean_reciprocal_rank - 0.75).abs() < f64::EPSILON);
    assert_eq!(report.by_category["temporal"].cases, 1);
}

#[test]
fn evaluation_rejects_ambiguous_contracts() {
    let valid = case("one", "category", &["a"]);
    let duplicate = [valid.clone(), valid.clone()];
    assert!(evaluate_retrieval(&duplicate, &[], &[1]).is_err());
    assert!(evaluate_retrieval(std::slice::from_ref(&valid), &[], &[0]).is_err());
    assert!(
        evaluate_retrieval(
            &[valid],
            &[RankedPrediction {
                case_id: "unknown".to_owned(),
                ranked_ids: Vec::new(),
            }],
            &[1],
        )
        .is_err()
    );

    let empty = EvaluationCase {
        id: "empty".to_owned(),
        category: "category".to_owned(),
        relevant_ids: BTreeSet::new(),
    };
    assert!(evaluate_retrieval(&[empty], &[], &[1]).is_err());
}
