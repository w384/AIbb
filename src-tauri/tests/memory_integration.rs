use aibb_desktop_pet_lib::{
    domain::Role,
    memory::{last_non_empty_paragraph, ContextBuilder, MemoryRepository},
    storage::Database,
};

#[test]
fn extracts_the_last_non_empty_paragraph() {
    let reply = "第一段。\n\n第二段。\n\n我还想出去玩，可以吗？\n";

    assert_eq!(
        last_non_empty_paragraph(reply),
        Some("我还想出去玩，可以吗？".to_string())
    );
}

#[test]
fn extracts_a_multiline_paragraph_after_whitespace_only_blank_lines() {
    let reply = "第一段。\r\n \r\n第二段第一行\r\n第二段第二行\r\n\r\n";

    assert_eq!(
        last_non_empty_paragraph(reply),
        Some("第二段第一行\n第二段第二行".to_string())
    );
}

#[tokio::test]
async fn context_survives_reopening_the_database() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("memory.sqlite3");
    {
        let repository = MemoryRepository::open(&path).unwrap();
        repository
            .append(Role::Assistant, "前文\n\n最后反馈")
            .await
            .unwrap();
    }

    let repository = MemoryRepository::open(&path).unwrap();
    let context = ContextBuilder::new(repository).build("继续").await.unwrap();

    assert_eq!(context.current_input, "继续");
    assert_eq!(
        context.last_assistant_paragraph.as_deref(),
        Some("最后反馈")
    );
}

#[tokio::test]
async fn context_injects_recent_messages_up_to_the_character_budget() {
    let directory = tempfile::tempdir().unwrap();
    let repository = MemoryRepository::open(directory.path().join("memory.sqlite3")).unwrap();

    for index in 0..45 {
        let (role, content) = if index % 2 == 0 {
            (Role::User, format!("user-{index}"))
        } else {
            (
                Role::Assistant,
                format!("assistant opening\n\nassistant-{index}"),
            )
        };
        repository.append(role, content).await.unwrap();
    }

    let context = ContextBuilder::new(repository)
        .build("current")
        .await
        .unwrap();

    // 内容远小于 128K 字符预算 → 全部注入，按时间正序。
    assert_eq!(context.recent_messages.len(), 45);
    assert_eq!(context.recent_messages[0].content, "user-0");
    assert_eq!(context.recent_messages[44].content, "user-44");
    assert_eq!(
        context.last_assistant_paragraph.as_deref(),
        Some("assistant-43")
    );
}

#[tokio::test]
async fn summary_candidate_requires_old_content_beyond_the_context_window() {
    let directory = tempfile::tempdir().unwrap();
    let exact_repository = MemoryRepository::open(directory.path().join("exact.sqlite3")).unwrap();
    exact_repository
        .append(Role::User, "界".repeat(12_000))
        .await
        .unwrap();
    for index in 0..40 {
        exact_repository
            .append(Role::Assistant, format!("recent-{index}"))
            .await
            .unwrap();
    }

    // 旧内容仍在 128K 上下文窗口内 → 不产生摘要候选。
    assert!(ContextBuilder::new(exact_repository)
        .summary_candidate()
        .await
        .unwrap()
        .is_none());

    let above_repository = MemoryRepository::open(directory.path().join("above.sqlite3")).unwrap();
    let old_message = above_repository
        .append(Role::User, "🙂".repeat(131_000))
        .await
        .unwrap();
    for index in 0..40 {
        above_repository
            .append(Role::Assistant, format!("recent-{index}"))
            .await
            .unwrap();
    }

    // 旧内容（131K 字符）超出上下文窗口 → 成为候选，且超过 12K 阈值触发。
    let candidate = ContextBuilder::new(above_repository.clone())
        .summary_candidate()
        .await
        .unwrap()
        .unwrap();

    assert_eq!(candidate.total_characters, 131_000);
    assert_eq!(candidate.messages, vec![old_message]);
    assert_eq!(
        above_repository.recent_messages(100).await.unwrap().len(),
        41,
        "building a summary candidate must retain all message rows"
    );
}

#[tokio::test]
async fn saved_summary_marks_but_retains_messages_and_survives_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("memory.sqlite3");
    let repository = MemoryRepository::open(&path).unwrap();
    let old_message = repository
        .append(Role::User, "old".repeat(44_000))
        .await
        .unwrap();
    for index in 0..40 {
        repository
            .append(Role::Assistant, format!("recent-{index}"))
            .await
            .unwrap();
    }
    let builder = ContextBuilder::new(repository.clone());
    let candidate = builder.summary_candidate().await.unwrap().unwrap();

    repository
        .save_summary(&candidate, "较早对话的持久摘要")
        .await
        .unwrap();

    assert!(builder.summary_candidate().await.unwrap().is_none());
    let all_messages = repository.recent_messages(100).await.unwrap();
    assert_eq!(all_messages.len(), 41);
    assert_eq!(all_messages[0].id, old_message.id);
    assert!(all_messages[0].summarized_at.is_some());
    assert!(all_messages[1..]
        .iter()
        .all(|message| message.summarized_at.is_none()));

    drop(builder);
    drop(repository);
    let reopened = MemoryRepository::open(&path).unwrap();
    let context = ContextBuilder::new(reopened).build("继续").await.unwrap();

    assert_eq!(context.summary.as_deref(), Some("较早对话的持久摘要"));
    // 上下文窗口按 128K 字符注入：40 条近期消息全部在窗口内。
    assert_eq!(context.recent_messages.len(), 40);
}

#[tokio::test]
async fn clear_memory_is_atomic_and_preserves_application_settings() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("memory.sqlite3");
    let database = Database::open(&path).unwrap();
    database
        .save_settings(
            "https://settings.example/v1",
            "kept-model",
            "force",
            false,
            true,
            "persisted-persona",
            "",
        )
        .unwrap();
    let repository = MemoryRepository::new(database.clone());
    repository
        .append(Role::User, "old".repeat(44_000))
        .await
        .unwrap();
    for index in 0..40 {
        repository
            .append(Role::Assistant, format!("recent-{index}"))
            .await
            .unwrap();
    }
    let candidate = ContextBuilder::new(repository.clone())
        .summary_candidate()
        .await
        .unwrap()
        .unwrap();
    repository
        .save_summary(&candidate, "summary")
        .await
        .unwrap();

    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute(
        "INSERT INTO explorations(id, status, created_at, updated_at) \
         VALUES ('exploration-1', 'running', 1, 1)",
        [],
    )
    .unwrap();
    raw.execute_batch(
        "CREATE TRIGGER reject_exploration_clear BEFORE DELETE ON explorations \
         BEGIN SELECT RAISE(FAIL, 'keep transaction atomic'); END;",
    )
    .unwrap();

    assert!(repository.clear_memory().await.is_err());
    assert_eq!(table_count(&raw, "messages"), 41);
    assert_eq!(table_count(&raw, "memory_summaries"), 1);
    assert_eq!(table_count(&raw, "explorations"), 1);

    raw.execute_batch("DROP TRIGGER reject_exploration_clear;")
        .unwrap();
    repository.clear_memory().await.unwrap();

    assert_eq!(table_count(&raw, "messages"), 0);
    assert_eq!(table_count(&raw, "memory_summaries"), 0);
    assert_eq!(table_count(&raw, "explorations"), 0);
    let settings = database.load_settings().unwrap();
    assert_eq!(settings.api_base, "https://settings.example/v1");
    assert_eq!(settings.model, "kept-model");
    assert_eq!(settings.web_mode, "force");
    assert!(!settings.always_on_top);
    assert!(settings.autostart);
}

fn table_count(connection: &rusqlite::Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}
