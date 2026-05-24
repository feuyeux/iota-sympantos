//! SQLite store layer.
//!
//! - [`cache`]    — [`CacheStore`]: execution lifecycle
//! - [`approval`] — [`ApprovalStore`]: tool approval events and policy
//! - [`ledger`]   — [`SessionLedger`]: sessions, backend sessions, turns, handoffs

pub mod approvals;
pub mod cache;
pub mod db;
pub mod observability;

pub mod ledger;

#[cfg(test)]
mod observability_tests;
