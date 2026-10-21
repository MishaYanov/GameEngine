use super::*;
#[test]
fn persistence_order_and_missing_links() {
    let path = std::env::temp_dir().join(format!("board-{}", now_ms()));
    let mut board = Board::new(&path);
    let draft = |title: &str| Draft {
        title: title.into(),
        description: "details".into(),
        priority: "Normal".into(),
        component: "renderer".into(),
        column: Column::Backlog,
        session_id: Some("session".into()),
    };
    let first = board.save(None, draft("first")).unwrap();
    let second = board.save(None, draft("second")).unwrap();
    board.link(&first.id, "expired-job").unwrap();
    board
        .move_card(&second.id, Column::Backlog, Some(&first.id))
        .unwrap();
    let restored = Board::new(&path);
    assert_eq!(restored.cards[0].id, second.id);
    assert_eq!(restored.cards[1].jobs, vec!["expired-job"]);
    board.move_card(&first.id, Column::Done, None).unwrap();
    board.delete(&second.id).unwrap();
    assert_eq!(Board::new(&path).cards.len(), 1);
}
#[test]
fn failed_save_keeps_memory() {
    let path = std::env::temp_dir().join(format!("board-fail-{}", now_ms()));
    std::fs::create_dir_all(path.join("board.json")).unwrap();
    let mut board = Board::new(&path);
    assert!(
        board
            .save(
                None,
                Draft {
                    title: "Task".into(),
                    description: String::new(),
                    priority: "Normal".into(),
                    component: String::new(),
                    column: Column::Backlog,
                    session_id: None
                }
            )
            .is_err()
    );
    assert!(board.cards.is_empty());
}
