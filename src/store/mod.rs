//! SQLite store layer.
//!
//! - [`cache`]    — [`CacheStore`]: execution lifecycle
//! - [`approval`] — [`ApprovalStore`]: tool approval events and policy
//! - [`ledger`]   — [`SessionLedger`]: sessions, backend sessions, turns, handoffs

pub mod approval;
pub mod cache;

pub mod ledger;
