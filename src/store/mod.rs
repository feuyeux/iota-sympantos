//! SQLite store layer.
//!
//! - [`memory`]   — [`MemoryStore`]: episodic/semantic/procedural memory with FTS5
//! - [`events`]   — [`EventStore`]: execution records, runtime events, Prometheus metrics
//! - [`approval`] — [`ApprovalStore`]: tool approval audit log and policy
//! - [`ledger`]   — [`SessionLedger`]: sessions, backend sessions, turns, handoffs

pub mod approval;
pub mod embedding;
pub mod events;
pub mod ledger;
pub mod memory;
