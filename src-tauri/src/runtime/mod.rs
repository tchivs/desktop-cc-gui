use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tokio::process::Child;
use tokio::sync::Mutex;
#[cfg(unix)]
use tokio::time::sleep;

use crate::backend::app_server::WorkspaceSession;
use crate::state::AppState;
use crate::types::{AppSettings, WorkspaceEntry};

const LEDGER_FILE_NAME: &str = "runtime-pool-ledger.json";
const HOT_IDLE_SECONDS: u64 = 30;
const TERMINATE_GRACE_MILLIS: u64 = 150;

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn write_json_atomically(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temp_path = path.with_extension(format!(
        "{}.tmp",
        uuid::Uuid::new_v4()
    ));
    fs::write(&temp_path, content).map_err(|error| error.to_string())?;
    fs::rename(&temp_path, path).map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RuntimeState {
    Starting,
    Hot,
    Warm,
    Busy,
    Stopping,
    Failed,
    ZombieSuspected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimePoolRow {
    pub(crate) workspace_id: String,
    pub(crate) workspace_name: String,
    pub(crate) workspace_path: String,
    pub(crate) engine: String,
    pub(crate) state: RuntimeState,
    pub(crate) pid: Option<u32>,
    pub(crate) wrapper_kind: Option<String>,
    pub(crate) resolved_bin: Option<String>,
    pub(crate) started_at_ms: Option<u64>,
    pub(crate) last_used_at_ms: u64,
    pub(crate) pinned: bool,
    pub(crate) lease_sources: Vec<String>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimePoolDiagnostics {
    pub(crate) orphan_entries_found: u32,
    pub(crate) orphan_entries_cleaned: u32,
    pub(crate) orphan_entries_failed: u32,
    pub(crate) force_kill_count: u32,
    pub(crate) last_orphan_sweep_at_ms: Option<u64>,
    pub(crate) last_shutdown_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimePoolBudgetSnapshot {
    pub(crate) max_hot_codex: u8,
    pub(crate) max_warm_codex: u8,
    pub(crate) warm_ttl_seconds: u16,
    pub(crate) restore_threads_only_on_launch: bool,
    pub(crate) force_cleanup_on_exit: bool,
    pub(crate) orphan_sweep_on_launch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimePoolSummary {
    pub(crate) total_runtimes: usize,
    pub(crate) hot_runtimes: usize,
    pub(crate) warm_runtimes: usize,
    pub(crate) busy_runtimes: usize,
    pub(crate) pinned_runtimes: usize,
    pub(crate) codex_runtimes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimePoolSnapshot {
    pub(crate) rows: Vec<RuntimePoolRow>,
    pub(crate) summary: RuntimePoolSummary,
    pub(crate) budgets: RuntimePoolBudgetSnapshot,
    pub(crate) diagnostics: RuntimePoolDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedRuntimeLedger {
    rows: Vec<RuntimePoolRow>,
    diagnostics: RuntimePoolDiagnostics,
}

#[derive(Debug, Clone)]
struct RuntimeEntry {
    workspace_id: String,
    workspace_name: String,
    workspace_path: String,
    engine: String,
    pid: Option<u32>,
    wrapper_kind: Option<String>,
    resolved_bin: Option<String>,
    started_at_ms: Option<u64>,
    last_used_at_ms: u64,
    pinned: bool,
    lease_sources: Vec<String>,
    error: Option<String>,
    busy: bool,
    session_exists: bool,
    zombie_suspected: bool,
}

impl RuntimeEntry {
    fn from_workspace(entry: &WorkspaceEntry, engine: &str, lease_source: &str) -> Self {
        Self {
            workspace_id: entry.id.clone(),
            workspace_name: entry.name.clone(),
            workspace_path: entry.path.clone(),
            engine: engine.to_string(),
            pid: None,
            wrapper_kind: None,
            resolved_bin: None,
            started_at_ms: None,
            last_used_at_ms: now_millis(),
            pinned: false,
            lease_sources: vec![lease_source.to_string()],
            error: None,
            busy: false,
            session_exists: false,
            zombie_suspected: false,
        }
    }
}

pub(crate) struct RuntimeManager {
    entries: Mutex<HashMap<String, RuntimeEntry>>,
    diagnostics: Mutex<RuntimePoolDiagnostics>,
    ledger_path: PathBuf,
    shutting_down: AtomicBool,
}

impl RuntimeManager {
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            diagnostics: Mutex::new(RuntimePoolDiagnostics::default()),
            ledger_path: data_dir.join(LEDGER_FILE_NAME),
            shutting_down: AtomicBool::new(false),
        }
    }

    pub(crate) fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }

    pub(crate) fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
    }

    pub(crate) async fn record_starting(
        &self,
        entry: &WorkspaceEntry,
        engine: &str,
        lease_source: &str,
    ) {
        let mut entries = self.entries.lock().await;
        let existing = entries
            .entry(entry.id.clone())
            .or_insert_with(|| RuntimeEntry::from_workspace(entry, engine, lease_source));
        existing.workspace_name = entry.name.clone();
        existing.workspace_path = entry.path.clone();
        existing.engine = engine.to_string();
        existing.last_used_at_ms = now_millis();
        existing.session_exists = false;
        existing.busy = false;
        existing.zombie_suspected = false;
        if !existing
            .lease_sources
            .iter()
            .any(|value| value == lease_source)
        {
            existing.lease_sources.push(lease_source.to_string());
        }
        drop(entries);
        let _ = self.persist_ledger().await;
    }

    pub(crate) async fn record_ready(
        &self,
        session: &WorkspaceSession,
        lease_source: &str,
    ) {
        let pid = {
            let child = session.child.lock().await;
            child.id()
        };
        let mut entries = self.entries.lock().await;
        let runtime = entries
            .entry(session.entry.id.clone())
            .or_insert_with(|| RuntimeEntry::from_workspace(&session.entry, "codex", lease_source));
        runtime.workspace_name = session.entry.name.clone();
        runtime.workspace_path = session.entry.path.clone();
        runtime.engine = "codex".to_string();
        runtime.pid = pid;
        runtime.wrapper_kind = Some(session.wrapper_kind.clone());
        runtime.resolved_bin = Some(session.resolved_bin.clone());
        runtime.started_at_ms.get_or_insert_with(now_millis);
        runtime.last_used_at_ms = now_millis();
        runtime.error = None;
        runtime.busy = false;
        runtime.session_exists = true;
        runtime.zombie_suspected = false;
        if !runtime
            .lease_sources
            .iter()
            .any(|value| value == lease_source)
        {
            runtime.lease_sources.push(lease_source.to_string());
        }
        drop(entries);
        let _ = self.persist_ledger().await;
    }

    pub(crate) async fn touch(
        &self,
        workspace_id: &str,
        lease_source: &str,
        busy: bool,
    ) {
        let mut entries = self.entries.lock().await;
        if let Some(runtime) = entries.get_mut(workspace_id) {
            runtime.last_used_at_ms = now_millis();
            runtime.busy = busy;
            runtime.session_exists = true;
            if !runtime
                .lease_sources
                .iter()
                .any(|value| value == lease_source)
            {
                runtime.lease_sources.push(lease_source.to_string());
            }
        }
        drop(entries);
        let _ = self.persist_ledger().await;
    }

    pub(crate) async fn pin_runtime(&self, workspace_id: &str, pinned: bool) {
        let mut entries = self.entries.lock().await;
        if let Some(runtime) = entries.get_mut(workspace_id) {
            runtime.pinned = pinned;
            runtime.last_used_at_ms = now_millis();
        }
        drop(entries);
        let _ = self.persist_ledger().await;
    }

    pub(crate) async fn record_failure(
        &self,
        entry: &WorkspaceEntry,
        engine: &str,
        lease_source: &str,
        error: String,
    ) {
        let mut entries = self.entries.lock().await;
        let runtime = entries
            .entry(entry.id.clone())
            .or_insert_with(|| RuntimeEntry::from_workspace(entry, engine, lease_source));
        runtime.error = Some(error);
        runtime.last_used_at_ms = now_millis();
        runtime.busy = false;
        runtime.session_exists = false;
        runtime.zombie_suspected = false;
        drop(entries);
        let _ = self.persist_ledger().await;
    }

    pub(crate) async fn record_stopping(&self, workspace_id: &str) {
        let mut entries = self.entries.lock().await;
        if let Some(runtime) = entries.get_mut(workspace_id) {
            runtime.busy = false;
            runtime.session_exists = false;
        }
        drop(entries);
        let _ = self.persist_ledger().await;
    }

    pub(crate) async fn record_removed(&self, workspace_id: &str) {
        let mut entries = self.entries.lock().await;
        entries.remove(workspace_id);
        drop(entries);
        let _ = self.persist_ledger().await;
    }

    pub(crate) async fn snapshot(&self, settings: &AppSettings) -> RuntimePoolSnapshot {
        let rows = self.snapshot_rows(settings).await;
        let summary = RuntimePoolSummary {
            total_runtimes: rows.len(),
            hot_runtimes: rows
                .iter()
                .filter(|row| matches!(row.state, RuntimeState::Hot))
                .count(),
            warm_runtimes: rows
                .iter()
                .filter(|row| matches!(row.state, RuntimeState::Warm))
                .count(),
            busy_runtimes: rows
                .iter()
                .filter(|row| matches!(row.state, RuntimeState::Busy))
                .count(),
            pinned_runtimes: rows.iter().filter(|row| row.pinned).count(),
            codex_runtimes: rows.iter().filter(|row| row.engine == "codex").count(),
        };
        RuntimePoolSnapshot {
            rows,
            summary,
            budgets: RuntimePoolBudgetSnapshot {
                max_hot_codex: settings.codex_max_hot_runtimes,
                max_warm_codex: settings.codex_max_warm_runtimes,
                warm_ttl_seconds: settings.codex_warm_ttl_seconds,
                restore_threads_only_on_launch: settings.runtime_restore_threads_only_on_launch,
                force_cleanup_on_exit: settings.runtime_force_cleanup_on_exit,
                orphan_sweep_on_launch: settings.runtime_orphan_sweep_on_launch,
            },
            diagnostics: self.diagnostics.lock().await.clone(),
        }
    }

    async fn snapshot_rows(&self, settings: &AppSettings) -> Vec<RuntimePoolRow> {
        let now = now_millis();
        let hot_cutoff_ms = HOT_IDLE_SECONDS.saturating_mul(1000);
        let mut rows = self
            .entries
            .lock()
            .await
            .values()
            .cloned()
            .map(|entry| {
                let age_ms = now.saturating_sub(entry.last_used_at_ms);
                let state = if entry.zombie_suspected {
                    RuntimeState::ZombieSuspected
                } else if entry.error.is_some() {
                    RuntimeState::Failed
                } else if entry.busy {
                    RuntimeState::Busy
                } else if age_ms <= hot_cutoff_ms {
                    RuntimeState::Hot
                } else {
                    RuntimeState::Warm
                };
                RuntimePoolRow {
                    workspace_id: entry.workspace_id,
                    workspace_name: entry.workspace_name,
                    workspace_path: entry.workspace_path,
                    engine: entry.engine,
                    state,
                    pid: entry.pid,
                    wrapper_kind: entry.wrapper_kind,
                    resolved_bin: entry.resolved_bin,
                    started_at_ms: entry.started_at_ms,
                    last_used_at_ms: entry.last_used_at_ms,
                    pinned: entry.pinned,
                    lease_sources: entry.lease_sources,
                    error: entry.error,
                }
            })
            .collect::<Vec<_>>();

        rows.sort_by(|left, right| right.last_used_at_ms.cmp(&left.last_used_at_ms));

        let hot_limit = settings.codex_max_hot_runtimes as usize;
        let warm_limit = settings.codex_max_warm_runtimes as usize;
        let mut idle_rank = 0usize;
        for row in &mut rows {
            if row.engine != "codex" || row.pinned || matches!(row.state, RuntimeState::Busy) {
                continue;
            }
            match row.state {
                RuntimeState::Hot | RuntimeState::Warm => {
                    if idle_rank < hot_limit {
                        row.state = RuntimeState::Hot;
                    } else if idle_rank < hot_limit + warm_limit {
                        row.state = RuntimeState::Warm;
                    } else {
                        row.state = RuntimeState::Warm;
                    }
                    idle_rank += 1;
                }
                _ => {}
            }
        }
        rows
    }

    pub(crate) async fn reconcile_pool(
        &self,
        settings: &AppSettings,
        sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    ) {
        let snapshot_rows = self.snapshot_rows(settings).await;
        let now = now_millis();
        let warm_ttl_ms = (settings.codex_warm_ttl_seconds as u64).saturating_mul(1000);
        let hot_limit = settings.codex_max_hot_runtimes as usize;
        let warm_limit = settings.codex_max_warm_runtimes as usize;
        let mut keep_idle = 0usize;
        let mut evict_workspace_ids = Vec::new();
        for row in snapshot_rows {
            if row.engine != "codex" || row.pinned || matches!(row.state, RuntimeState::Busy) {
                continue;
            }
            if now.saturating_sub(row.last_used_at_ms) > warm_ttl_ms {
                evict_workspace_ids.push(row.workspace_id.clone());
                continue;
            }
            if keep_idle < hot_limit + warm_limit {
                keep_idle += 1;
                continue;
            }
            evict_workspace_ids.push(row.workspace_id.clone());
        }
        for workspace_id in evict_workspace_ids {
            let _ = stop_workspace_session(sessions, self, &workspace_id).await;
        }
    }

    async fn persist_ledger(&self) -> Result<(), String> {
        let rows = self
            .entries
            .lock()
            .await
            .values()
            .filter(|entry| entry.pid.is_some() || entry.error.is_some())
            .cloned()
            .map(|entry| RuntimePoolRow {
                workspace_id: entry.workspace_id,
                workspace_name: entry.workspace_name,
                workspace_path: entry.workspace_path,
                engine: entry.engine,
                state: if entry.zombie_suspected {
                    RuntimeState::ZombieSuspected
                } else if entry.error.is_some() {
                    RuntimeState::Failed
                } else if entry.busy {
                    RuntimeState::Busy
                } else {
                    RuntimeState::Warm
                },
                pid: entry.pid,
                wrapper_kind: entry.wrapper_kind,
                resolved_bin: entry.resolved_bin,
                started_at_ms: entry.started_at_ms,
                last_used_at_ms: entry.last_used_at_ms,
                pinned: entry.pinned,
                lease_sources: entry.lease_sources,
                error: entry.error,
            })
            .collect::<Vec<_>>();
        let diagnostics = self.diagnostics.lock().await.clone();
        let payload = serde_json::to_string_pretty(&PersistedRuntimeLedger { rows, diagnostics })
            .map_err(|error| error.to_string())?;
        write_json_atomically(&self.ledger_path, &payload)
    }

    pub(crate) fn orphan_sweep_on_startup(&self, enabled: bool) {
        if !enabled {
            return;
        }
        let raw_ledger = match fs::read_to_string(&self.ledger_path) {
            Ok(raw_ledger) => raw_ledger,
            Err(_) => return,
        };
        let parsed = match serde_json::from_str::<PersistedRuntimeLedger>(&raw_ledger) {
            Ok(parsed) => parsed,
            Err(_) => return,
        };
        let mut diagnostics = parsed.diagnostics;
        diagnostics.last_orphan_sweep_at_ms = Some(now_millis());
        diagnostics.orphan_entries_found += parsed.rows.len() as u32;
        for row in parsed.rows {
            let Some(pid) = row.pid else {
                continue;
            };
            match terminate_pid_tree(pid) {
                Ok(force_killed) => {
                    diagnostics.orphan_entries_cleaned += 1;
                    if force_killed {
                        diagnostics.force_kill_count += 1;
                    }
                }
                Err(_) => diagnostics.orphan_entries_failed += 1,
            }
        }
        let payload = PersistedRuntimeLedger {
            rows: Vec::new(),
            diagnostics: diagnostics.clone(),
        };
        if let Ok(serialized) = serde_json::to_string_pretty(&payload) {
            let _ = write_json_atomically(&self.ledger_path, &serialized);
        }
        *self.diagnostics.blocking_lock() = diagnostics;
    }

    pub(crate) async fn set_diagnostics(&self, diagnostics: RuntimePoolDiagnostics) {
        *self.diagnostics.lock().await = diagnostics;
        let _ = self.persist_ledger().await;
    }

    pub(crate) async fn note_force_kill(&self) {
        let mut diagnostics = self.diagnostics.lock().await;
        diagnostics.force_kill_count += 1;
        drop(diagnostics);
        let _ = self.persist_ledger().await;
    }

    pub(crate) async fn note_shutdown(&self) {
        let mut diagnostics = self.diagnostics.lock().await;
        diagnostics.last_shutdown_at_ms = Some(now_millis());
        drop(diagnostics);
        let _ = self.persist_ledger().await;
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "action")]
pub(crate) enum RuntimePoolMutation {
    Close {
        #[serde(alias = "workspaceId")]
        workspace_id: String,
    },
    ReleaseToCold {
        #[serde(alias = "workspaceId")]
        workspace_id: String,
    },
    Pin {
        #[serde(alias = "workspaceId")]
        workspace_id: String,
        pinned: bool,
    },
}

#[tauri::command]
pub(crate) async fn ensure_runtime_ready(
    workspace_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let entry = {
        let workspaces = state.workspaces.lock().await;
        workspaces
            .get(&workspace_id)
            .cloned()
            .ok_or_else(|| "workspace not found".to_string())?
    };
    if entry
        .settings
        .engine_type
        .as_deref()
        .map(|value| !value.eq_ignore_ascii_case("codex"))
        .unwrap_or(false)
    {
        return Ok(());
    }
    crate::codex::ensure_codex_session(&workspace_id, &state, &app).await?;
    let settings = state.app_settings.lock().await.clone();
    state
        .runtime_manager
        .reconcile_pool(&settings, &state.sessions)
        .await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn get_runtime_pool_snapshot(
    state: State<'_, AppState>,
) -> Result<RuntimePoolSnapshot, String> {
    let settings = state.app_settings.lock().await.clone();
    Ok(state.runtime_manager.snapshot(&settings).await)
}

#[tauri::command]
pub(crate) async fn mutate_runtime_pool(
    mutation: RuntimePoolMutation,
    state: State<'_, AppState>,
) -> Result<RuntimePoolSnapshot, String> {
    match mutation {
        RuntimePoolMutation::Close { workspace_id }
        | RuntimePoolMutation::ReleaseToCold { workspace_id } => {
            stop_workspace_session(&state.sessions, &state.runtime_manager, &workspace_id).await?;
        }
        RuntimePoolMutation::Pin {
            workspace_id,
            pinned,
        } => {
            state.runtime_manager.pin_runtime(&workspace_id, pinned).await;
        }
    }
    let settings = state.app_settings.lock().await.clone();
    state
        .runtime_manager
        .reconcile_pool(&settings, &state.sessions)
        .await;
    Ok(state.runtime_manager.snapshot(&settings).await)
}

pub(crate) async fn terminate_workspace_session_process(child: &mut Child) -> Result<bool, String> {
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            let process_group_id = pid as libc::pid_t;
            let terminate_status = unsafe { libc::kill(-process_group_id, libc::SIGTERM) };
            if terminate_status == 0 {
                sleep(Duration::from_millis(TERMINATE_GRACE_MILLIS)).await;
                if matches!(child.try_wait(), Ok(Some(_))) {
                    let _ = child.wait().await;
                    return Ok(false);
                }
            }
            let kill_status = unsafe { libc::kill(-process_group_id, libc::SIGKILL) };
            if kill_status == 0 {
                let _ = child.wait().await;
                return Ok(true);
            }
        }
    }

    #[cfg(windows)]
    {
        if let Some(pid) = child.id() {
            let output = crate::utils::async_command("taskkill")
                .arg("/PID")
                .arg(pid.to_string())
                .arg("/T")
                .arg("/F")
                .output()
                .await
                .map_err(|error| format!("taskkill failed for pid {pid}: {error}"))?;
            if output.status.success() || matches!(child.try_wait(), Ok(Some(_))) {
                let _ = child.wait().await;
                return Ok(true);
            }
        }
    }

    child
        .kill()
        .await
        .map_err(|error| format!("Failed to kill process: {error}"))?;
    let _ = child.wait().await;
    Ok(true)
}

pub(crate) async fn terminate_workspace_session(
    session: Arc<WorkspaceSession>,
    runtime_manager: Option<&RuntimeManager>,
) -> Result<(), String> {
    let workspace_id = session.entry.id.clone();
    if let Some(runtime_manager) = runtime_manager {
        runtime_manager.record_stopping(&workspace_id).await;
    }
    let forced = {
        let mut child = session.child.lock().await;
        terminate_workspace_session_process(&mut child).await?
    };
    if forced {
        if let Some(runtime_manager) = runtime_manager {
            runtime_manager.note_force_kill().await;
        }
    }
    if let Some(runtime_manager) = runtime_manager {
        runtime_manager.record_removed(&workspace_id).await;
    }
    Ok(())
}

pub(crate) async fn replace_workspace_session(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    runtime_manager: Option<&RuntimeManager>,
    workspace_id: String,
    new_session: Arc<WorkspaceSession>,
    lease_source: &str,
) -> Result<(), String> {
    if let Some(runtime_manager) = runtime_manager {
        runtime_manager.record_ready(&new_session, lease_source).await;
    }
    let old_session = sessions.lock().await.insert(workspace_id, new_session);
    if let Some(old_session) = old_session {
        terminate_workspace_session(old_session, runtime_manager).await?;
    }
    Ok(())
}

pub(crate) async fn stop_workspace_session(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    runtime_manager: &RuntimeManager,
    workspace_id: &str,
) -> Result<(), String> {
    let session = sessions.lock().await.remove(workspace_id);
    if let Some(session) = session {
        terminate_workspace_session(session, Some(runtime_manager)).await?;
    } else {
        runtime_manager.record_removed(workspace_id).await;
    }
    Ok(())
}

pub(crate) async fn shutdown_managed_runtimes(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    runtime_manager: &RuntimeManager,
) {
    runtime_manager.begin_shutdown();
    let active_sessions = {
        let mut sessions = sessions.lock().await;
        sessions.drain().map(|(_, session)| session).collect::<Vec<_>>()
    };
    for session in active_sessions {
        let _ = terminate_workspace_session(session, Some(runtime_manager)).await;
    }
    runtime_manager.note_shutdown().await;
}

fn terminate_pid_tree(pid: u32) -> Result<bool, String> {
    #[cfg(windows)]
    {
        let status = crate::utils::std_command("taskkill")
            .arg("/PID")
            .arg(pid.to_string())
            .arg("/T")
            .arg("/F")
            .status()
            .map_err(|error| error.to_string())?;
        return Ok(status.success());
    }

    #[cfg(unix)]
    {
        let pgid = pid as libc::pid_t;
        let terminate_status = unsafe { libc::kill(-pgid, libc::SIGTERM) };
        if terminate_status == 0 {
            std::thread::sleep(Duration::from_millis(TERMINATE_GRACE_MILLIS));
        }
        let kill_status = unsafe { libc::kill(-pgid, libc::SIGKILL) };
        if kill_status != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error.to_string());
            }
        }
        return Ok(true);
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimeEntry, RuntimeManager, RuntimeState};
    use crate::types::{AppSettings, WorkspaceEntry, WorkspaceKind, WorkspaceSettings};

    fn workspace_entry(id: &str) -> WorkspaceEntry {
        let mut settings = WorkspaceSettings::default();
        settings.engine_type = Some("codex".to_string());
        WorkspaceEntry {
            id: id.to_string(),
            name: format!("Workspace {id}"),
            path: format!("/tmp/{id}"),
            codex_bin: None,
            kind: WorkspaceKind::Main,
            parent_id: None,
            worktree: None,
            settings,
        }
    }

    #[tokio::test]
    async fn snapshot_applies_hot_and_warm_budget() {
        let manager = RuntimeManager::new(&std::env::temp_dir());
        let entry_a = workspace_entry("a");
        let entry_b = workspace_entry("b");
        let entry_c = workspace_entry("c");
        manager.record_starting(&entry_a, "codex", "test").await;
        manager.record_starting(&entry_b, "codex", "test").await;
        manager.record_starting(&entry_c, "codex", "test").await;
        {
            let mut entries = manager.entries.lock().await;
            let now = super::now_millis();
            for (offset, key) in ["a", "b", "c"].iter().enumerate() {
                let entry = entries.get_mut(*key).expect("entry exists");
                entry.started_at_ms = Some(now);
                entry.session_exists = true;
                entry.last_used_at_ms = now.saturating_sub((offset as u64) * 45_000);
            }
        }
        let mut settings = AppSettings::default();
        settings.codex_max_hot_runtimes = 1;
        settings.codex_max_warm_runtimes = 1;
        let snapshot = manager.snapshot(&settings).await;
        assert_eq!(snapshot.rows.len(), 3);
        assert!(matches!(snapshot.rows[0].state, RuntimeState::Hot));
        assert!(matches!(snapshot.rows[1].state, RuntimeState::Warm));
    }

    #[test]
    fn runtime_entry_from_workspace_sets_initial_lease_source() {
        let entry = RuntimeEntry::from_workspace(&workspace_entry("abc"), "codex", "connect");
        assert_eq!(entry.lease_sources, vec!["connect".to_string()]);
        assert_eq!(entry.engine, "codex");
    }
}
