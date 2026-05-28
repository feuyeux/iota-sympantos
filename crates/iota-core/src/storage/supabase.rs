//! Supabase REST API client for pipeline artifact persistence.
//!
//! Each write operation is wrapped with exponential-backoff retry so
//! transient network errors do not break the pipeline. All operations
//! emit structured error logs on final failure.
//!
//! Supabase is configured via the `SUPABASE_URL` and `SUPABASE_ANON_KEY`
//! environment variables (or via the `NIMIA_SUPABASE_*` aliases).

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::time::Duration;

use super::models::{PipelineArtifact, PipelineRecord};
use super::retry::with_backoff;

const MAX_RETRIES: u32 = 3;
const BASE_DELAY: Duration = Duration::from_secs(2);

/// Configured Supabase client wrapping a blocking `reqwest::Client`.
#[derive(Clone)]
pub struct SupabaseStore {
    client: Client,
    url: reqwest::Url,
    anon_key: String,
}

/// Standard Supabase REST error envelope.
#[derive(Debug, Deserialize)]
pub struct SbError {
    pub message: String,
    #[serde(rename = "error")]
    pub error_code: Option<String>,
}

/// Insert response shape — Supabase returns the inserted row.
#[derive(Debug, Deserialize)]
pub struct InsertResponse<T> {
    #[serde(flatten)]
    pub data: T,
}

impl SupabaseStore {
    /// Create a new client from the environment.
    ///
    /// Reads `SUPABASE_URL` / `SUPABASE_ANON_KEY` first, then falls back
    /// to `NIMIA_SUPABASE_URL` / `NIMIA_SUPABASE_ANON_KEY`.
    pub fn from_env() -> Result<Self> {
        let url = std::env::var("SUPABASE_URL")
            .or_else(|_| std::env::var("NIMIA_SUPABASE_URL"))
            .context("SUPABASE_URL not set — set SUPABASE_URL or NIMIA_SUPABASE_URL")?;
        let anon_key = std::env::var("SUPABASE_ANON_KEY")
            .or_else(|_| std::env::var("NIMIA_SUPABASE_ANON_KEY"))
            .context(
                "SUPABASE_ANON_KEY not set — set SUPABASE_ANON_KEY or NIMIA_SUPABASE_ANON_KEY",
            )?;
        Self::new(url, anon_key)
    }

    /// Create a client with an explicit URL + anon key.
    pub fn new(url: String, anon_key: String) -> Result<Self> {
        let url = reqwest::Url::parse(&url).context("invalid SUPABASE_URL")?;
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            client,
            url,
            anon_key,
        })
    }

    /// Build the `Authorization: Bearer <anon_key>` header.
    fn bearer(&self) -> String {
        format!("Bearer {}", self.anon_key)
    }

    /// Issue a POST to the given table with `body` as JSON.
    fn post<T: DeserializeOwned>(&self, table: &str, body: impl serde::Serialize) -> Result<T> {
        with_backoff(
            || {
                let url = self.url.join(&format!("rest/v1/{}", table)).unwrap();
                let resp = self
                    .client
                    .post(url)
                    .header("Authorization", self.bearer())
                    .header("apikey", &self.anon_key)
                    .header("Content-Type", "application/json")
                    .header("Prefer", "return=representation")
                    .json(&body)
                    .send()
                    .context("POST request failed")?;
                if resp.status().is_success() || resp.status().as_u16() == 201 {
                    resp.json().context("failed to parse POST response")
                } else {
                    let status = resp.status();
                    let msg = resp.text().unwrap_or_default();
                    let err: SbError = serde_json::from_str(&msg).unwrap_or(SbError {
                        message: msg,
                        error_code: None,
                    });
                    Err(anyhow::anyhow!(
                        "Supabase POST {}: {} ({:?}) [status {}]",
                        table,
                        err.message,
                        err.error_code,
                        status
                    ))
                }
            },
            MAX_RETRIES,
            BASE_DELAY,
        )
    }

    // --------------------------------------------------------------------------
    // High-level pipeline operations
    // --------------------------------------------------------------------------

    /// Store any pipeline artifact (research / script / x_optimizer).
    ///
    /// Internally this inserts a row in the `pipeline_records` table.
    pub fn store_artifact(&self, artifact: PipelineArtifact) -> Result<PipelineRecord> {
        let record = artifact.into_record();
        let _: InsertResponse<PipelineRecord> = self
            .post("pipeline_records", &record)
            .context("store_artifact failed after retries")?;
        tracing::info!(
            id = %record.id,
            stage = %record.stage,
            "pipeline artifact stored to Supabase"
        );
        Ok(record)
    }

    /// Fetch a pipeline record by its UUID.
    pub fn get_record(&self, id: uuid::Uuid) -> Result<PipelineRecord> {
        with_backoff(
            || {
                let url = self
                    .url
                    .join(&format!("rest/v1/pipeline_records?id=eq.{}", id))
                    .unwrap();
                let resp = self
                    .client
                    .get(url)
                    .header("Authorization", self.bearer())
                    .header("apikey", &self.anon_key)
                    .header("Accept", "application/json")
                    .send()
                    .context("GET request failed")?;
                if resp.status().is_success() {
                    let rows: Vec<PipelineRecord> =
                        resp.json().context("failed to parse GET response")?;
                    rows.into_iter()
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("record {} not found", id))
                } else {
                    Err(anyhow::anyhow!(
                        "GET pipeline_records/{}: {}",
                        id,
                        resp.status()
                    ))
                }
            },
            MAX_RETRIES,
            BASE_DELAY,
        )
    }

    /// List the most recent N records for a given stage.
    pub fn list_by_stage(&self, stage: &str, limit: usize) -> Result<Vec<PipelineRecord>> {
        with_backoff(
            || {
                let url = self
                    .url
                    .join(&format!(
                        "rest/v1/pipeline_records?stage=eq.{}&order=created_at.desc&limit={}",
                        stage, limit
                    ))
                    .unwrap();
                let resp = self
                    .client
                    .get(url)
                    .header("Authorization", self.bearer())
                    .header("apikey", &self.anon_key)
                    .header("Accept", "application/json")
                    .send()
                    .context("GET request failed")?;
                if resp.status().is_success() {
                    let rows: Vec<PipelineRecord> =
                        resp.json().context("failed to parse GET response")?;
                    Ok(rows)
                } else {
                    Err(anyhow::anyhow!(
                        "GET pipeline_records by stage: {}",
                        resp.status()
                    ))
                }
            },
            MAX_RETRIES,
            BASE_DELAY,
        )
    }
}
