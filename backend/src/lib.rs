//! shimau — a tiny, modern Docker Compose manager.
//!
//! The application is a thin, typed layer over the Compose CLI: Compose files
//! stay the source of truth, the filesystem stays the source of truth, and
//! the only state shimau owns is its own administrator account.
//!
//! The binary in `main.rs` is a thin wrapper over this library so the HTTP
//! surface can be exercised end to end from integration tests.

pub mod api;
pub mod auth;
pub mod compose;
pub mod config;
pub mod db;
pub mod error;
pub mod ops;
pub mod stacks;
