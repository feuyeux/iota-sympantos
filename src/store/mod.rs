//! SQLite store layer.
//!
//! - [`memory`]   — [`MemoryStore`]: episodic/semantic/procedural memory with FTS5
//! - [`cache`]    — [`CacheStore`]: execution replay, join-running, dedupe
//! - [`approval`] — [`ApprovalStore`]: tool approval events and policy
//! - [`ledger`]   — [`SessionLedger`]: sessions, backend sessions, turns, handoffs

pub mod approval;
pub mod cache;
pub mod embedding;

pub mod ledger;
pub mod memory;
