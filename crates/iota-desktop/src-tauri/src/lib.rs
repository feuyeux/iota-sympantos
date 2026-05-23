use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{oneshot, Mutex};

use iota_core::acp::{permission::ApprovalRequest, AcpBackend};
use iota_core::config::{read_config, NimiaConfig};
use iota_core::engine::IotaEngine;
use iota_core::kanban::{types::*, KanbanStore, SqliteKanbanStore};

pub struct AppState {
    pub kanban_store: Arc<Mutex<SqliteKanbanStore>>,
    pub shadows_dir: PathBuf,
    pub engines: Arc<std::sync::Mutex<BTreeMap<PathBuf, Arc<Mutex<IotaEngine>>>>>,
    pub pending_approvals: Arc<std::sync::Mutex<BTreeMap<String, oneshot::Sender<bool>>>>,
}

fn get_or_create_engine(
    state: &tauri::State<'_, AppState>,
    cwd: PathBuf,
) -> Result<Arc<Mutex<IotaEngine>>, String> {
    let mut engines = state.engines.lock().unwrap();
    if let Some(engine) = engines.get(&cwd) {
        return Ok(engine.clone());
    }

    let config = read_config().map_err(|e| e.to_string())?;
    let engine = Arc::new(Mutex::new(IotaEngine::create_session(
        config,
        false, // show_native
        600_000,
        Some(&cwd),
    )));
    engines.insert(cwd.clone(), engine.clone());
    Ok(engine)
}

fn save_config_file(config: &NimiaConfig) -> Result<(), String> {
    let path = iota_core::config::config_path().map_err(|e| e.to_string())?;
    let content = serde_yaml::to_string(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(())
}

#[allow(clippy::collapsible_if)]
fn is_api_key_configured(config: &NimiaConfig, backend: AcpBackend) -> bool {
    let backend_cfg = match backend {
        AcpBackend::ClaudeCode => config.claude_code.as_ref(),
        AcpBackend::Codex => config.codex.as_ref(),
        AcpBackend::Gemini => config.gemini.as_ref(),
        AcpBackend::Hermes => config.hermes.as_ref(),
        AcpBackend::OpenCode => config.opencode.as_ref(),
    };

    if let Some(cfg) = backend_cfg {
        if let Some(ref model_cfg) = cfg.model {
            if let Some(ref key) = model_cfg.api_key {
                return !key.is_empty() && key != "<api-key>" && key != "YOUR_API_KEY";
            }
        }
    }

    // Fallback to environment variables
    let env_var = match backend {
        AcpBackend::ClaudeCode => std::env::var("ANTHROPIC_API_KEY"),
        AcpBackend::Gemini => std::env::var("GEMINI_API_KEY"),
        AcpBackend::Codex => std::env::var("OPENAI_API_KEY"),
        AcpBackend::Hermes => std::env::var("HERMES_API_KEY"),
        AcpBackend::OpenCode => std::env::var("OPENCODE_API_KEY"),
    };
    if let Ok(val) = env_var {
        if !val.is_empty() {
            return true;
        }
    }

    false
}

#[tauri::command]
fn get_config() -> Result<NimiaConfig, String> {
    read_config().map_err(|e| e.to_string())
}

#[tauri::command]
fn save_api_key(
    backend_str: String,
    api_key: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let backend = AcpBackend::parse(&backend_str).map_err(|e| format!("Invalid backend: {}", e))?;
    let mut config = read_config().map_err(|e| e.to_string())?;

    let update_model_key = |cfg: &mut Option<iota_core::config::BackendConfig>| {
        let mut inner = cfg.clone().unwrap_or_default();
        let mut model = inner.model.clone().unwrap_or_default();
        model.api_key = Some(api_key.clone());
        inner.model = Some(model);
        *cfg = Some(inner);
    };

    match backend {
        AcpBackend::ClaudeCode => update_model_key(&mut config.claude_code),
        AcpBackend::Codex => update_model_key(&mut config.codex),
        AcpBackend::Gemini => update_model_key(&mut config.gemini),
        AcpBackend::Hermes => update_model_key(&mut config.hermes),
        AcpBackend::OpenCode => update_model_key(&mut config.opencode),
    }

    save_config_file(&config)?;

    // Force recreate the cached engine for this cwd to pick up the new config
    let home = dirs::home_dir().ok_or_else(|| "Could not find home directory".to_string())?;
    let cwd = std::env::current_dir().unwrap_or(home);
    let mut engines = state.engines.lock().unwrap();
    engines.remove(&cwd);

    Ok(())
}

#[tauri::command]
async fn submit_prompt(
    prompt: String,
    backend_str: String,
    state: tauri::State<'_, AppState>,
    window: tauri::Window,
) -> Result<(), String> {
    let backend = AcpBackend::parse(&backend_str).map_err(|e| format!("Invalid backend: {}", e))?;

    // Check if API key is configured
    let config = read_config().map_err(|e| e.to_string())?;
    if !is_api_key_configured(&config, backend) {
        return Err("API_KEY_REQUIRED".to_string());
    }

    let home = dirs::home_dir().ok_or_else(|| "Could not find home directory".to_string())?;
    let cwd = std::env::current_dir().unwrap_or(home);

    let engine = get_or_create_engine(&state, cwd.clone())?;

    let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel::<String>(100);
    let (approval_tx, mut approval_rx) = tokio::sync::mpsc::channel::<ApprovalRequest>(10);

    // 1. Install TUI approval channel (Tauri shares this interface)
    iota_core::acp::permission::install_tui_approval_channel(approval_tx).await;

    // 2. Spawn listener for approvals
    let pending_approvals = state.pending_approvals.clone();
    let window_approval = window.clone();
    tokio::spawn(async move {
        while let Some(req) = approval_rx.recv().await {
            let req_id = uuid::Uuid::new_v4().to_string();
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

            pending_approvals
                .lock()
                .unwrap()
                .insert(req_id.clone(), reply_tx);

            let _ = window_approval.emit(
                "approval-requested",
                serde_json::json!({
                    "id": req_id,
                    "tool_name": req.tool_name,
                    "params": req.params
                }),
            );

            if let Ok(decision) = reply_rx.await {
                let _ = req.reply.send(decision);
            } else {
                let _ = req.reply.send(false);
            }
        }
    });

    // 3. Spawn listener for stream chunks
    let window_stream = window.clone();
    tokio::spawn(async move {
        while let Some(chunk) = stream_rx.recv().await {
            let _ = window_stream.emit("chat-stream-chunk", chunk);
        }
    });

    // 4. Run execution in background tokio task
    let engine_clone = engine.clone();
    tokio::spawn(async move {
        let mut engine = engine_clone.lock().await;
        engine.set_stream_output_sender(Some(stream_tx));
        let result = engine.run_with_timing(backend, cwd, &prompt).await;
        engine.set_stream_output_sender(None);

        match result {
            Ok(output) => {
                let _ = window.emit(
                    "chat-complete",
                    serde_json::json!({
                        "text": output.text,
                        "events": output.events,
                        "timing": output.timing
                    }),
                );
            }
            Err(e) => {
                let _ = window.emit("chat-error", e.to_string());
            }
        }
    });

    Ok(())
}

#[tauri::command]
fn handle_approval(
    req_id: String,
    approved: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut pending = state.pending_approvals.lock().unwrap();
    if let Some(tx) = pending.remove(&req_id) {
        let _ = tx.send(approved);
        Ok(())
    } else {
        Err("Approval request not found or already processed".to_string())
    }
}

#[tauri::command]
async fn list_boards(state: tauri::State<'_, AppState>) -> Result<Vec<Board>, String> {
    let store = state.kanban_store.lock().await;
    store.list_boards().map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_tasks(
    filter: TaskFilter,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Task>, String> {
    let store = state.kanban_store.lock().await;
    store.list_tasks(filter).map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_task(
    req: CreateTaskRequest,
    state: tauri::State<'_, AppState>,
) -> Result<TaskId, String> {
    let store = state.kanban_store.lock().await;
    store.create_task(req).map_err(|e| e.to_string())
}

#[tauri::command]
async fn transition_task(
    task_id: TaskId,
    to_status: Status,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let store = state.kanban_store.lock().await;
    store
        .transition(task_id, to_status)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_comments(
    task_id: TaskId,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Comment>, String> {
    let store = state.kanban_store.lock().await;
    store.list_comments(task_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_comment(
    task_id: TaskId,
    author: String,
    body: String,
    state: tauri::State<'_, AppState>,
) -> Result<CommentId, String> {
    let store = state.kanban_store.lock().await;
    store
        .add_comment(task_id, &author, &body)
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::collapsible_if)]
pub fn run() {
    let home = dirs::home_dir().expect("could not find home directory");
    let kanban_dir = home.join(".i6").join("kanban");
    std::fs::create_dir_all(&kanban_dir).expect("failed to create kanban directory");

    let store_path = kanban_dir.join("iota.db");
    let shadows_dir = kanban_dir.join("shadows");
    std::fs::create_dir_all(&shadows_dir).expect("failed to create shadows directory");

    let store = SqliteKanbanStore::open(&store_path).expect("failed to open sqlite store");

    // Seed initial board and tasks if database is empty
    if let Ok(true) = store.list_boards().map(|b| b.is_empty()) {
        if let Ok(board_id) = store.create_board("iota-proj", "Iota Sympantos Core") {
            let _ = store.create_task(CreateTaskRequest {
                board_id,
                title: "配置 ACP 代理后端 (Gemini / Claude)".to_string(),
                body: Some("配置 nimia.yaml，验证各 AI 后端的连接与自动权限机制。".to_string()),
                status: Some(Status::Todo),
                assignee: Some("Developer".to_string()),
                priority: Some(1),
                tags: vec!["configuration".to_string(), "backend".to_string()],
                workspace_kind: None,
                workspace_path: None,
            });
            let _ = store.create_task(CreateTaskRequest {
                board_id,
                title: "开发 Tauri 桌面应用大盘".to_string(),
                body: Some(
                    "使用 React + Tailwind CSS v4 开发 iota-desktop 前端视图与交互组件。"
                        .to_string(),
                ),
                status: Some(Status::Ready),
                assignee: Some("Developer".to_string()),
                priority: Some(2),
                tags: vec!["frontend".to_string(), "gui".to_string()],
                workspace_kind: None,
                workspace_path: None,
            });
            let _ = store.create_task(CreateTaskRequest {
                board_id,
                title: "实现多节点事件同步协议".to_string(),
                body: Some("编写基于 TCP / HTTP 的 Kanban 变更事件同步实现。".to_string()),
                status: Some(Status::Triage),
                assignee: None,
                priority: Some(3),
                tags: vec!["sync".to_string(), "networking".to_string()],
                workspace_kind: None,
                workspace_path: None,
            });
            let _ = store.create_task(CreateTaskRequest {
                board_id,
                title: "完成 CLI/TUI 的 Rust 模块拆分".to_string(),
                body: Some(
                    "将单一包拆分为 iota-core 与 iota-cli 并通过 Cargo 统一编排。".to_string(),
                ),
                status: Some(Status::Done),
                assignee: Some("System".to_string()),
                priority: Some(0),
                tags: vec!["refactor".to_string(), "core".to_string()],
                workspace_kind: None,
                workspace_path: None,
            });
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            kanban_store: Arc::new(Mutex::new(store)),
            shadows_dir,
            engines: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
            pending_approvals: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_api_key,
            submit_prompt,
            handle_approval,
            list_boards,
            list_tasks,
            create_task,
            transition_task,
            list_comments,
            add_comment
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

