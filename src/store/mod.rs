//! SQLite store layer.
//!
//! - [`cache`]    — [`CacheStore`]: execution lifecycle
//! - [`approval`] — [`ApprovalStore`]: tool approval events and policy
//! - [`ledger`]   — [`SessionLedger`]: sessions, backend sessions, turns, handoffs

pub mod approvals;
pub mod cache;

pub mod ledger;
