//! AIbb file archive: the second-phase "drag a file onto AIbb to archive it"
//! feature ported from the harness `local_drop` prototype. Rust owns the
//! controlled archive chain (receive -> classify -> version -> back up ->
//! place + SQLite ledger); the renderer only hands over dropped paths.

pub mod engine;
pub mod models;
pub mod rules;
pub mod service;
pub mod structure_lib;
