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

pub fn apply(connection: &mut Connection) -> Result<(), rusqlite_migration::Error> {
    Migrations::new(vec![M::up(INITIAL_SCHEMA), M::up(DEEPSEEK_MODEL_BACKFILL)])
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
}
