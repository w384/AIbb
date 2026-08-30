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

pub fn apply(connection: &mut Connection) -> Result<(), rusqlite_migration::Error> {
    Migrations::new(vec![M::up(INITIAL_SCHEMA)]).to_latest(connection)
}
