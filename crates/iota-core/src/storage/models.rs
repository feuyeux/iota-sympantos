//! Domain types for the storage layer.
//!
//! Defines the unified schema used across all three pipeline stages
//! (Research Agent topic data, Script Agent script content, X Optimizer
//! optimization posts) so downstream queries are consistent.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unified enum covering all storable pipeline artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum PipelineArtifact {
    /// Research Agent — raw/finalized topic data.
    Research(ResearchData),
    /// Script Agent — produced video script content.
    Script(ScriptData),
    /// X Optimizer Agent — optimized post + metadata.
    XOptimizer(XOptimizerData),
}

/// Common fields shared by every pipeline record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRecord {
    pub id: Uuid,
    pub stage: String,
    pub content: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: PipelineStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStatus {
    Pending,
    Stored,
    Failed,
}

impl PipelineRecord {
    pub fn new(stage: &str, content: serde_json::Value) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            stage: stage.to_string(),
            content,
            created_at: now,
            updated_at: now,
            status: PipelineStatus::Pending,
        }
    }
}

// ---------------------------------------------------------------------------
// Stage-specific data structures
// ---------------------------------------------------------------------------

/// Research Agent output: topic data selected for video production.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchData {
    pub topic_source: String,
    pub heat_score: u8,
    pub topic: String,
    pub metadata: serde_json::Value,
}

/// Script Agent output: full script content ready for X optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptData {
    pub title: String,
    pub voiceover_script: serde_json::Value,
    pub sections: serde_json::Value,
    pub format: String,
    pub output_path: String,
    pub metadata: serde_json::Value,
}

/// X Optimizer Agent output: optimized X/Twitter post + associated metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XOptimizerData {
    pub post_text: String,
    pub hashtags: Vec<String>,
    pub media_urls: Vec<String>,
    pub metadata: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

impl PipelineArtifact {
    pub fn research(data: ResearchData) -> Self {
        PipelineArtifact::Research(data)
    }

    pub fn script(data: ScriptData) -> Self {
        PipelineArtifact::Script(data)
    }

    pub fn x_optimizer(data: XOptimizerData) -> Self {
        PipelineArtifact::XOptimizer(data)
    }

    /// Convert into a full [`PipelineRecord`] using the variant name as stage.
    pub fn into_record(self) -> PipelineRecord {
        let stage = match &self {
            PipelineArtifact::Research(_) => "research",
            PipelineArtifact::Script(_) => "script",
            PipelineArtifact::XOptimizer(_) => "x_optimizer",
        }
        .to_string();
        PipelineRecord::new(&stage, serde_json::to_value(&self).unwrap())
    }
}
