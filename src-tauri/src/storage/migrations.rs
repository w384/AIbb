use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

const INITIAL_SCHEMA: &str = r#"
CREATE TABLE app_settings (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  api_base TEXT NOT NULL DEFAULT '',
  model TEXT NOT NULL DEFAULT '',
  web_mode TEXT NOT NULL DEFAULT 'auto'
    CHECK (web_mode IN ('auto', 'force', 'off')),
  always_on_top INTEGER NOT NULL DEFAULT 1,
  autostart INTEGER NOT NULL DEFAULT 0,
  first_run_complete INTEGER NOT NULL DEFAULT 0,
  pet_x INTEGER,
  pet_y INTEGER
);

INSERT INTO app_settings(singleton) VALUES (1);

CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
  content TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  summarized_at INTEGER
);

CREATE INDEX messages_created_at_idx ON messages(created_at);

CREATE TABLE memory_summaries (
  id TEXT PRIMARY KEY,
  content TEXT NOT NULL,
  through_message_created_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE explorations (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  user_direction TEXT,
  items_json TEXT,
  next_outing_request TEXT,
  raw_response TEXT,
  error_code TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
"#;

const DEEPSEEK_MODEL_BACKFILL: &str = r#"
UPDATE app_settings
SET model = 'deepseek-v4-flash'
WHERE TRIM(model) = ''
  AND LOWER(TRIM(api_base)) IN (
    'https://api.deepseek.com',
    'https://api.deepseek.com/',
    'https://api.deepseek.com/v1',
    'https://api.deepseek.com/v1/'
  );
"#;

const AIBB_PROFILE: &str = r#"
ALTER TABLE app_settings ADD COLUMN aibb_name TEXT NOT NULL DEFAULT 'AIbb';
ALTER TABLE app_settings ADD COLUMN avatar_filename TEXT;
ALTER TABLE app_settings ADD COLUMN profile_version INTEGER NOT NULL DEFAULT 0;
"#;

const OUTING_DIARIES: &str = r#"
ALTER TABLE explorations ADD COLUMN diary TEXT;
ALTER TABLE explorations ADD COLUMN sources_json TEXT;
ALTER TABLE explorations ADD COLUMN round_number INTEGER;
ALTER TABLE explorations ADD COLUMN elapsed_seconds INTEGER;
"#;

const OUTING_UNIQUENESS: &str = r#"
UPDATE explorations
SET status = 'interrupted', updated_at = updated_at + 1
WHERE status IN (
  'queued', 'choosing', 'native_searching', 'public_searching',
  'reading', 'writing', 'correcting'
)
AND rowid NOT IN (
  SELECT rowid FROM explorations
  WHERE status IN (
    'queued', 'choosing', 'native_searching', 'public_searching',
    'reading', 'writing', 'correcting'
  )
  ORDER BY created_at DESC, rowid DESC
  LIMIT 1
);

WITH ranked AS (
  SELECT rowid,
         ROW_NUMBER() OVER (ORDER BY created_at ASC, rowid ASC) AS round_number
  FROM explorations
  WHERE status = 'completed'
)
UPDATE explorations
SET round_number = (
  SELECT ranked.round_number FROM ranked WHERE ranked.rowid = explorations.rowid
)
WHERE status = 'completed';

CREATE UNIQUE INDEX explorations_one_active_idx
ON explorations((1))
WHERE status IN (
  'queued', 'choosing', 'native_searching', 'public_searching',
  'reading', 'writing', 'correcting'
);

CREATE UNIQUE INDEX explorations_round_number_idx
ON explorations(round_number)
WHERE status = 'completed' AND round_number IS NOT NULL;
"#;

const ARCHIVE_FEATURE: &str = r#"
ALTER TABLE app_settings ADD COLUMN archive_root TEXT NOT NULL DEFAULT '';
ALTER TABLE app_settings ADD COLUMN archive_auto_discover INTEGER NOT NULL DEFAULT 1;

CREATE TABLE archive_structure_lib (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  template_name TEXT NOT NULL DEFAULT '默认',
  templates_json TEXT NOT NULL
);

CREATE TABLE archive_ledger (
  id TEXT PRIMARY KEY,
  file_name TEXT NOT NULL,
  project TEXT NOT NULL,
  category TEXT NOT NULL,
  period TEXT NOT NULL,
  version TEXT NOT NULL,
  archive_rel_path TEXT NOT NULL,
  backup_rel_path TEXT,
  status TEXT NOT NULL,
  error_code TEXT,
  created_at INTEGER NOT NULL
);

CREATE INDEX archive_ledger_created_idx ON archive_ledger(created_at);
"#;

pub fn apply(connection: &mut Connection) -> Result<(), rusqlite_migration::Error> {
    Migrations::new(vec![
        M::up(INITIAL_SCHEMA),
        M::up(DEEPSEEK_MODEL_BACKFILL),
        M::up(AIBB_PROFILE),
        M::up(OUTING_DIARIES),
        M::up(OUTING_UNIQUENESS),
        M::up(ARCHIVE_FEATURE),
    ])
    .to_latest(connection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrades_a_blank_deepseek_model_to_the_real_default() {
        let mut connection = Connection::open_in_memory().unwrap();
        Migrations::new(vec![M::up(INITIAL_SCHEMA)])
            .to_latest(&mut connection)
            .unwrap();
        connection
            .execute(
                "UPDATE app_settings SET api_base = ?1, model = '' WHERE singleton = 1",
                ["https://api.deepseek.com"],
            )
            .unwrap();

        apply(&mut connection).unwrap();

        let model: String = connection
            .query_row(
                "SELECT model FROM app_settings WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(model, "deepseek-v4-flash");
    }

    #[test]
    fn adds_profile_columns_with_the_expected_defaults() {
        let mut connection = Connection::open_in_memory().unwrap();
        Migrations::new(vec![M::up(INITIAL_SCHEMA)])
            .to_latest(&mut connection)
            .unwrap();

        apply(&mut connection).unwrap();

        let profile = connection
            .query_row(
                "SELECT aibb_name, avatar_filename, profile_version \
                 FROM app_settings WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(profile, ("AIbb".into(), None, 0));
    }

    #[test]
    fn repairs_legacy_duplicates_before_enforcing_outing_uniqueness() {
        let mut connection = Connection::open_in_memory().unwrap();
        Migrations::new(vec![
            M::up(INITIAL_SCHEMA),
            M::up(DEEPSEEK_MODEL_BACKFILL),
            M::up(AIBB_PROFILE),
            M::up(OUTING_DIARIES),
        ])
        .to_latest(&mut connection)
        .unwrap();
        connection
            .execute_batch(
                "INSERT INTO explorations(id, status, created_at, updated_at) VALUES
                   ('active-old', 'reading', 1, 1),
                   ('active-new', 'writing', 2, 2),
                   ('done-old', 'completed', 3, 3),
                   ('done-new', 'completed', 4, 4);
                 UPDATE explorations SET round_number = 1 WHERE status = 'completed';",
            )
            .unwrap();

        apply(&mut connection).unwrap();

        let active_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM explorations WHERE status IN
                   ('queued', 'choosing', 'native_searching', 'public_searching',
                    'reading', 'writing', 'correcting')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let rounds = connection
            .prepare(
                "SELECT round_number FROM explorations WHERE status = 'completed'
                 ORDER BY created_at, rowid",
            )
            .unwrap()
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(active_count, 1);
        assert_eq!(rounds, vec![1, 2]);
        assert!(connection
            .execute(
                "INSERT INTO explorations(id, status, created_at, updated_at)
                 VALUES ('active-third', 'queued', 5, 5)",
                [],
            )
            .is_err());
    }
}
