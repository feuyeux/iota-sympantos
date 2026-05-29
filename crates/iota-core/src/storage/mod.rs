//! Storage layer — Supabase persistence for pipeline artifacts.
//!
//! ## Schema (Supabase SQL)
//!
//! Run the following in the Supabase SQL editor to create the required table:
//!
//! ```sql
//! create table if not exists pipeline_records (
//!   id          uuid    primary key default gen_random_uuid(),
//!   stage       text    not null,  -- 'research' | 'script' | 'x_optimizer'
//!   content     jsonb   not null,
//!   status      text    not null default 'pending',  -- 'pending' | 'stored' | 'failed'
//!   created_at  timestamptz not null default now(),
//!   updated_at  timestamptz not null default now()
//! );
//!
//! create index if not exists pipeline_records_stage_idx
//!   on pipeline_records (stage, created_at desc);
//! ```
//!
//! ## Environment variables
//!
//! | Variable             | Alias               | Description            |
//! | :------------------- | :------------------ | :--------------------- |
//! | `SUPABASE_URL`       | `NIMIA_SUPABASE_URL` | Supabase project URL   |
//! | `SUPABASE_ANON_KEY`  | `NIMIA_SUPABASE_ANON_KEY` | Anon key for REST API |
//!
//! ## Usage
//!
//! ```rust,ignore
//! let store = SupabaseStore::from_env()?;
//! store.store_artifact(PipelineArtifact::script(ScriptData { .. }))?;
//! ```
//!
//! ## Retry behaviour
//!
//! All network operations use exponential back-off (3 retries, 2 s base delay).
//! On exhausted retries the error is logged and returned as `Err`.

pub mod models;
pub mod retry;
pub mod supabase;

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;

pub use models::{
    PipelineArtifact, PipelineRecord, PipelineStatus, ResearchData, ScriptData, XOptimizerData,
};
pub use supabase::SupabaseStore;
