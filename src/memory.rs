use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryType {
    Semantic,
    Episodic,
    Procedural,
}

impl MemoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Episodic => "episodic",
            Self::Procedural => "procedural",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryFacet {
    Identity,
    Preference,
    Strategic,
    Domain,
}

impl MemoryFacet {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Preference => "preference",
            Self::Strategic => "strategic",
            Self::Domain => "domain",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryScope {
    Session,
    Project,
    User,
    Global,
}

impl MemoryScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Project => "project",
            Self::User => "user",
            Self::Global => "global",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub memory_type: String,
    pub facet: Option<String>,
    pub scope: String,
    pub scope_id: String,
    pub content: String,
    pub confidence: f64,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: i64,
}

#[derive(Clone)]
pub struct MemoryStore {
    conn: Arc<Mutex<Connection>>,
    fts_available: bool,
}

#[derive(Debug, Clone)]
pub struct MemoryInsert {
    pub memory_type: MemoryType,
    pub facet: Option<MemoryFacet>,
    pub scope: MemoryScope,
    pub scope_id: String,
    pub content: String,
    pub confidence: f64,
    pub source_backend: Option<String>,
    pub source_session_id: Option<String>,
    pub source_execution_id: Option<String>,
    pub metadata_json: Option<String>,
    pub ttl_days: i64,
    pub supersedes: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RecallBuckets {
    pub identity: Vec<MemoryRecord>,
    pub preference: Vec<MemoryRecord>,
    pub strategic: Vec<MemoryRecord>,
    pub domain: Vec<MemoryRecord>,
    pub procedural: Vec<MemoryRecord>,
    pub episodic: Vec<MemoryRecord>,
}

impl MemoryStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open memory store {}", path.display()))?;
        let conn = Arc::new(Mutex::new(conn));
        let store = Self {
            conn,
            fts_available: false,
        };
        let fts_available = store.init()?;
        Ok(Self {
            conn: store.conn,
            fts_available,
        })
    }

    pub fn default_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Failed to get home directory")?;
        Ok(home.join(".i6").join("context").join("memory.sqlite"))
    }

    pub fn insert(&self, input: MemoryInsert) -> Result<String> {
        validate_taxonomy(&input)?;
        let now = now_ts();
        let ttl_days = input.ttl_days.max(1);
        let expires_at = now + ttl_days * 86_400;
        let content_hash = content_hash(&input.content);
        let id = Uuid::new_v4().to_string();
        let conn = self.conn.lock().expect("memory store mutex poisoned");
        if let Some(existing_id) = conn
            .query_row(
                "SELECT id FROM memory WHERE scope = ?1 AND scope_id = ?2 AND type = ?3 AND facet IS ?4 AND content_hash = ?5 LIMIT 1",
                params![
                    input.scope.as_str(),
                    input.scope_id,
                    input.memory_type.as_str(),
                    input.facet.as_ref().map(MemoryFacet::as_str),
                    content_hash,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()? {
            conn.execute(
                "UPDATE memory SET updated_at = ?2, expires_at = ?3, confidence = MAX(confidence, ?4) WHERE id = ?1",
                params![existing_id, now, expires_at, input.confidence],
            )?;
            return Ok(existing_id);
        }
        let supersedes = input.supersedes.clone().or_else(|| {
            latest_related_memory_id(
                &conn,
                input.scope.as_str(),
                &input.scope_id,
                input.memory_type.as_str(),
                input.facet.as_ref().map(MemoryFacet::as_str),
            )
            .ok()
            .flatten()
        });
        conn.execute(
            "INSERT INTO memory (id, type, facet, scope, scope_id, content, content_hash, confidence, source_backend, source_session_id, source_execution_id, metadata_json, ttl_days, created_at, updated_at, expires_at, supersedes, owner, visibility)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14, ?15, ?16, 'local', 'private')
             ON CONFLICT(scope, scope_id, type, facet, content_hash) DO UPDATE SET updated_at=excluded.updated_at, expires_at=excluded.expires_at, confidence=MAX(memory.confidence, excluded.confidence)",
            params![
                id,
                input.memory_type.as_str(),
                input.facet.as_ref().map(MemoryFacet::as_str),
                input.scope.as_str(),
                input.scope_id,
                input.content,
                content_hash,
                input.confidence,
                input.source_backend,
                input.source_session_id,
                input.source_execution_id,
                input.metadata_json,
                ttl_days,
                now,
                expires_at,
                supersedes,
            ],
        )?;
        Ok(id)
    }

    pub fn recall_buckets(
        &self,
        user_id: &str,
        project_id: &str,
        session_id: &str,
    ) -> Result<RecallBuckets> {
        Ok(RecallBuckets {
            identity: self.query(
                MemoryScope::User,
                user_id,
                Some(MemoryFacet::Identity),
                Some(MemoryType::Semantic),
                0.85,
                20,
            )?,
            preference: self.query(
                MemoryScope::User,
                user_id,
                Some(MemoryFacet::Preference),
                Some(MemoryType::Semantic),
                0.80,
                30,
            )?,
            strategic: self.query(
                MemoryScope::Project,
                project_id,
                Some(MemoryFacet::Strategic),
                Some(MemoryType::Semantic),
                0.80,
                30,
            )?,
            domain: self.query(
                MemoryScope::Project,
                project_id,
                Some(MemoryFacet::Domain),
                Some(MemoryType::Semantic),
                0.80,
                50,
            )?,
            procedural: self.query(
                MemoryScope::Project,
                project_id,
                None,
                Some(MemoryType::Procedural),
                0.75,
                10,
            )?,
            episodic: self.query(
                MemoryScope::Session,
                session_id,
                None,
                Some(MemoryType::Episodic),
                0.70,
                20,
            )?,
        })
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let trimmed = query.trim();
        if self.fts_available && !trimmed.is_empty() {
            match self.search_fts(trimmed, limit) {
                Ok(records) => return Ok(records),
                Err(_) => {}
            }
        }
        self.search_like(trimmed, limit)
    }

    fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        let conn = self.conn.lock().expect("memory store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT m.id, m.type, m.facet, m.scope, m.scope_id, m.content, m.confidence, m.created_at, m.updated_at, m.expires_at
             FROM memory m JOIN memory_fts f ON m.rowid = f.rowid
             WHERE memory_fts MATCH ?1 AND m.expires_at > ?2
             ORDER BY rank, m.confidence DESC, m.updated_at DESC LIMIT ?3",
        )?;
        rows_to_records(stmt.query_map(
            params![query, now_ts(), limit as i64],
            row_to_memory_record,
        )?)
    }

    fn search_like(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        let needle = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let conn = self.conn.lock().expect("memory store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, type, facet, scope, scope_id, content, confidence, created_at, updated_at, expires_at FROM memory
             WHERE expires_at > ?1 AND content LIKE ?2 ESCAPE ?4
             ORDER BY confidence DESC, updated_at DESC LIMIT ?3",
        )?;
        rows_to_records(stmt.query_map(
            params![now_ts(), needle, limit as i64, "\\"],
            row_to_memory_record,
        )?)
    }

    fn query(
        &self,
        scope: MemoryScope,
        scope_id: &str,
        facet: Option<MemoryFacet>,
        memory_type: Option<MemoryType>,
        min_confidence: f64,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        let facet_value = facet.as_ref().map(MemoryFacet::as_str);
        let type_value = memory_type.as_ref().map(MemoryType::as_str);
        let conn = self.conn.lock().expect("memory store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, type, facet, scope, scope_id, content, confidence, created_at, updated_at, expires_at FROM memory\n             WHERE scope = ?1 AND scope_id = ?2 AND (?3 IS NULL OR facet = ?3) AND (?4 IS NULL OR type = ?4) AND confidence >= ?5 AND expires_at > ?6\n             ORDER BY confidence DESC, updated_at DESC, created_at DESC LIMIT ?7",
        )?;
        rows_to_records(stmt.query_map(
            params![
                scope.as_str(),
                scope_id,
                facet_value,
                type_value,
                min_confidence,
                now_ts(),
                limit as i64
            ],
            row_to_memory_record,
        )?)
    }

    fn init(&self) -> Result<bool> {
        let conn = self.conn.lock().expect("memory store mutex poisoned");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory (
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL CHECK(type IN ('semantic','episodic','procedural')),
  facet TEXT CHECK(facet IN ('identity','preference','strategic','domain')),
  scope TEXT NOT NULL CHECK(scope IN ('session','project','user','global')),
  scope_id TEXT NOT NULL,
  content TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  confidence REAL NOT NULL DEFAULT 1.0,
  source_backend TEXT,
  source_session_id TEXT,
  source_execution_id TEXT,
  metadata_json TEXT,
  ttl_days INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  supersedes TEXT,
  owner TEXT NOT NULL DEFAULT 'local',
  visibility TEXT NOT NULL DEFAULT 'private'
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_dedup ON memory(scope, scope_id, type, facet, content_hash);
CREATE INDEX IF NOT EXISTS idx_memory_recall_semantic ON memory(scope, scope_id, facet, confidence DESC, updated_at DESC) WHERE type = 'semantic';
CREATE INDEX IF NOT EXISTS idx_memory_recall_procedural ON memory(scope, scope_id, confidence DESC, updated_at DESC) WHERE type = 'procedural';
CREATE INDEX IF NOT EXISTS idx_memory_recall_episodic ON memory(scope, scope_id, created_at DESC) WHERE type = 'episodic';",
        )?;
        Ok(init_fts(&conn).is_ok())
    }
}

fn init_fts(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
  content,
  content='memory',
  content_rowid='rowid',
  tokenize='unicode61'
);
CREATE TRIGGER IF NOT EXISTS memory_ai AFTER INSERT ON memory BEGIN
  INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
END;
CREATE TRIGGER IF NOT EXISTS memory_ad AFTER DELETE ON memory BEGIN
  INSERT INTO memory_fts(memory_fts, rowid, content) VALUES('delete', old.rowid, old.content);
END;
CREATE TRIGGER IF NOT EXISTS memory_au AFTER UPDATE ON memory BEGIN
  INSERT INTO memory_fts(memory_fts, rowid, content) VALUES('delete', old.rowid, old.content);
  INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
END;"
    )?;
    let missing_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memory m LEFT JOIN memory_fts f ON m.rowid = f.rowid WHERE f.rowid IS NULL",
        [],
        |row| row.get(0),
    )?;
    if missing_count > 0 {
        conn.execute("INSERT INTO memory_fts(memory_fts) VALUES('rebuild')", [])?;
    }
    Ok(())
}

fn validate_taxonomy(input: &MemoryInsert) -> Result<()> {
    if input.memory_type == MemoryType::Semantic && input.facet.is_none() {
        bail!("semantic memory requires a facet");
    }
    if input.memory_type != MemoryType::Semantic && input.facet.is_some() {
        bail!("only semantic memory may set facet");
    }
    Ok(())
}

fn latest_related_memory_id(
    conn: &Connection,
    scope: &str,
    scope_id: &str,
    memory_type: &str,
    facet: Option<&str>,
) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM memory WHERE scope = ?1 AND scope_id = ?2 AND type = ?3 AND facet IS ?4 ORDER BY updated_at DESC LIMIT 1",
        params![scope, scope_id, memory_type, facet],
        |row| row.get(0),
    )
    .optional()
    .context("Failed to find related memory")
}

fn row_to_memory_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
    Ok(MemoryRecord {
        id: row.get(0)?,
        memory_type: row.get(1)?,
        facet: row.get(2)?,
        scope: row.get(3)?,
        scope_id: row.get(4)?,
        content: row.get(5)?,
        confidence: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        expires_at: row.get(9)?,
    })
}

fn rows_to_records(
    rows: impl Iterator<Item = rusqlite::Result<MemoryRecord>>,
) -> Result<Vec<MemoryRecord>> {
    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    Ok(records)
}

fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.trim().as_bytes());
    hex::encode(hasher.finalize())
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_memory_content() {
        let store = MemoryStore::open(Path::new(":memory:")).unwrap();
        let input = test_memory_insert(
            MemoryType::Semantic,
            Some(MemoryFacet::Identity),
            MemoryScope::User,
            "local",
            "User prefers concise answers",
        );
        store.insert(input.clone()).unwrap();
        store.insert(input).unwrap();
        let buckets = store.recall_buckets("local", "project", "session").unwrap();
        assert_eq!(buckets.identity.len(), 1);
    }

    #[test]
    fn search_finds_records_and_empty_query_lists_records() {
        let store = MemoryStore::open(Path::new(":memory:")).unwrap();
        store
            .insert(test_memory_insert(
                MemoryType::Semantic,
                Some(MemoryFacet::Domain),
                MemoryScope::Project,
                "project",
                "ACP uses JSON-RPC over newline-delimited stdio",
            ))
            .unwrap();
        store
            .insert(test_memory_insert(
                MemoryType::Procedural,
                None,
                MemoryScope::Project,
                "project",
                "Run cargo test before submitting changes",
            ))
            .unwrap();

        let records = store.search("JSON-RPC", 10).unwrap();
        assert!(records.iter().any(|record| record.content.contains("JSON-RPC")));

        let all = store.search("", 10).unwrap();
        assert_eq!(all.len(), 2);
        assert!(store.search("cargo", 0).unwrap().is_empty());
    }

    fn test_memory_insert(
        memory_type: MemoryType,
        facet: Option<MemoryFacet>,
        scope: MemoryScope,
        scope_id: &str,
        content: &str,
    ) -> MemoryInsert {
        MemoryInsert {
            memory_type,
            facet,
            scope,
            scope_id: scope_id.to_string(),
            content: content.to_string(),
            confidence: 1.0,
            source_backend: None,
            source_session_id: None,
            source_execution_id: None,
            metadata_json: None,
            ttl_days: 365,
            supersedes: None,
        }
    }
}
