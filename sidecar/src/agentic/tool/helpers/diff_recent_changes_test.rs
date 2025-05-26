use super::diff_recent_changes::{ChangeWeight, DiffFileContent, DiffRecentChanges};

#[test]
fn test_change_weight_calculation() {
    let weight = ChangeWeight::new(10, 8, 0.75, 7);
    let score = weight.calculate_score();
    assert!(score > 0.0);
    assert!(score <= 10.0);
}

#[test]
fn test_cache_promotion() {
    let file_content = DiffFileContent::new(
        "test.rs".to_string(),
        "content".to_string(),
        None,
        Some(ChangeWeight::new(10, 8, 0.75, 7)),
        1234567890,
    );

    let changes = DiffRecentChanges::new(
        "".to_string(),
        "".to_string(),
        vec![file_content.clone()],
        5.0,
    );

    assert!(changes.should_promote_to_l1(&file_content));
}

#[test]
fn test_cache_invalidation() {
    let mut file_content = DiffFileContent::new(
        "test.rs".to_string(),
        "content".to_string(),
        None,
        Some(ChangeWeight::new(10, 8, 0.75, 7)),
        1234567890,
    );

    assert!(!file_content.is_invalidated());
    file_content.invalidate();
    assert!(file_content.is_invalidated());
}

#[test]
fn test_weight_update() {
    let mut file_content = DiffFileContent::new(
        "test.rs".to_string(),
        "content".to_string(),
        None,
        None,
        1234567890,
    );

    let new_weight = ChangeWeight::new(5, 6, 0.5, 4);
    file_content.update_weight(new_weight.clone());

    assert!(file_content.change_weight.is_some());
    assert_eq!(
        file_content.change_weight.unwrap().calculate_score(),
        new_weight.calculate_score()
    );
}
