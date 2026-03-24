//! Gemini engine implementation
//!
//! Handles Gemini CLI execution via:
//! `gemini -p "<prompt>" --output-format stream-json`

use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, Mutex, RwLock};

use super::events::EngineEvent;
use super::gemini_history::{load_gemini_session, GeminiSessionMessage};
use super::{EngineConfig, EngineType, SendMessageParams};

#[derive(Debug, Clone)]
pub struct GeminiTurnEvent {
    pub turn_id: String,
    pub event: EngineEvent,
}

#[derive(Debug, Default)]
struct GeminiVendorRuntimeConfig {
    env: HashMap<String, String>,
    auth_mode: Option<String>,
}

/// Gemini session for a workspace
pub struct GeminiSession {
    pub workspace_id: String,
    pub workspace_path: PathBuf,
    session_id: RwLock<Option<String>>,
    event_sender: broadcast::Sender<GeminiTurnEvent>,
    bin_path: Option<String>,
    home_dir: Option<String>,
    custom_args: Option<String>,
    active_processes: Mutex<HashMap<String, Child>>,
    interrupted: AtomicBool,
}

impl GeminiSession {
    pub fn new(
        workspace_id: String,
        workspace_path: PathBuf,
        config: Option<EngineConfig>,
    ) -> Self {
        let (event_sender, _) = broadcast::channel(1024);
        let config = config.unwrap_or_default();
        Self {
            workspace_id,
            workspace_path,
            session_id: RwLock::new(None),
            event_sender,
            bin_path: config.bin_path,
            home_dir: config.home_dir,
            custom_args: config.custom_args,
            active_processes: Mutex::new(HashMap::new()),
            interrupted: AtomicBool::new(false),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<GeminiTurnEvent> {
        self.event_sender.subscribe()
    }

    pub async fn get_session_id(&self) -> Option<String> {
        self.session_id.read().await.clone()
    }

    async fn set_session_id(&self, id: Option<String>) {
        *self.session_id.write().await = id;
    }

    fn emit_turn_event(&self, turn_id: &str, event: EngineEvent) {
        let _ = self.event_sender.send(GeminiTurnEvent {
            turn_id: turn_id.to_string(),
            event,
        });
    }

    pub fn emit_error(&self, turn_id: &str, error: String) {
        self.emit_turn_event(
            turn_id,
            EngineEvent::TurnError {
                workspace_id: self.workspace_id.clone(),
                error,
                code: None,
            },
        );
    }

    fn with_external_spec_hint(text: &str, custom_spec_root: Option<&str>) -> String {
        let Some(spec_root) = custom_spec_root
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return text.to_string();
        };
        if !Path::new(spec_root).is_absolute() {
            return text.to_string();
        }
        format!(
            "[External OpenSpec Root]\n- Path: {spec_root}\n- Treat this as the active spec root when checking or reading project specs.\n[/External OpenSpec Root]\n\n{text}"
        )
    }

    fn normalize_image_path_for_prompt(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.starts_with("data:") {
            log::warn!(
                "Gemini image attachment is data-url based; Gemini CLI needs file paths, skipping"
            );
            return None;
        }
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            log::warn!(
                "Gemini image attachment is remote-url based; Gemini CLI needs local file paths, skipping: {}",
                trimmed
            );
            return None;
        }
        if let Some(path) = trimmed.strip_prefix("file://") {
            let path_without_host = path.strip_prefix("localhost/").unwrap_or(path);
            if cfg!(windows)
                && path_without_host.starts_with('/')
                && path_without_host
                    .as_bytes()
                    .get(2)
                    .is_some_and(|value| *value == b':')
            {
                return Some(path_without_host[1..].to_string());
            }
            return Some(path_without_host.to_string());
        }
        Some(trimmed.to_string())
    }

    fn escape_image_reference(path: &str) -> String {
        let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
        format!("@\"{}\"", escaped)
    }

    fn with_image_references(text: &str, images: Option<&[String]>) -> String {
        let Some(images) = images else {
            return text.to_string();
        };
        let mut image_references: Vec<String> = Vec::new();
        for raw in images {
            if let Some(path) = Self::normalize_image_path_for_prompt(raw) {
                let reference = Self::escape_image_reference(&path);
                if !image_references.iter().any(|existing| existing == &reference) {
                    image_references.push(reference);
                }
            }
        }
        if image_references.is_empty() {
            return text.to_string();
        }
        let mut merged = text.trim_end().to_string();
        if !merged.is_empty() {
            merged.push_str("\n\n");
        }
        merged.push_str(&image_references.join(" "));
        merged
    }

    fn normalize_auth_mode(raw_mode: Option<&str>) -> Option<&'static str> {
        let normalized = raw_mode
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase())?;
        match normalized.as_str() {
            "custom" => Some("custom"),
            "login_google" => Some("login_google"),
            "gemini_api_key" => Some("gemini_api_key"),
            "vertex_adc" => Some("vertex_adc"),
            "vertex_service_account" => Some("vertex_service_account"),
            "vertex_api_key" => Some("vertex_api_key"),
            _ => None,
        }
    }

    fn selected_auth_type_for_mode(raw_mode: Option<&str>) -> &'static str {
        match Self::normalize_auth_mode(raw_mode) {
            Some("login_google") => "oauth-personal",
            Some("vertex_adc") | Some("vertex_service_account") | Some("vertex_api_key") => {
                "vertex-ai"
            }
            Some("custom") | Some("gemini_api_key") => "gemini-api-key",
            _ => "oauth-personal",
        }
    }

    fn resolve_global_gemini_dir(home_override: Option<&str>) -> Option<PathBuf> {
        let Some(home) = dirs::home_dir() else {
            return None;
        };
        let override_path = home_override
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let Some(raw_root) = override_path.or(Some(home)) else {
            return None;
        };
        if raw_root
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value == ".gemini")
        {
            return Some(raw_root);
        }
        Some(raw_root.join(".gemini"))
    }

    fn persist_auth_mode_hint(auth_mode: Option<&str>, home_override: Option<&str>) {
        let Some(gemini_dir) = Self::resolve_global_gemini_dir(home_override) else {
            return;
        };
        let selected_type = Self::selected_auth_type_for_mode(auth_mode);
        let settings_path = gemini_dir.join("settings.json");
        let mut root = std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|content| serde_json::from_str::<Value>(&content).ok())
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();

        let security = root
            .entry("security".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !security.is_object() {
            *security = Value::Object(serde_json::Map::new());
        }
        let Some(security_obj) = security.as_object_mut() else {
            return;
        };

        let auth = security_obj
            .entry("auth".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !auth.is_object() {
            *auth = Value::Object(serde_json::Map::new());
        }
        let Some(auth_obj) = auth.as_object_mut() else {
            return;
        };
        auth_obj.insert(
            "selectedType".to_string(),
            Value::String(selected_type.to_string()),
        );
        auth_obj.insert("useExternal".to_string(), Value::Bool(false));

        if let Some(parent) = settings_path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                log::warn!(
                    "failed to ensure Gemini settings dir {}: {}",
                    parent.display(),
                    error
                );
                return;
            }
        }
        let content = match serde_json::to_string_pretty(&Value::Object(root)) {
            Ok(serialized) => serialized,
            Err(error) => {
                log::warn!("failed to serialize Gemini settings auth hint: {}", error);
                return;
            }
        };
        if let Err(error) = std::fs::write(&settings_path, content) {
            log::warn!(
                "failed to persist Gemini settings auth hint to {}: {}",
                settings_path.display(),
                error
            );
        }
    }

    fn apply_auth_mode_env_overrides(cmd: &mut Command, auth_mode: Option<&str>) {
        match Self::normalize_auth_mode(auth_mode) {
            Some("login_google") => {
                cmd.env("GOOGLE_GENAI_USE_GCA", "true");
                cmd.env_remove("GOOGLE_GENAI_USE_VERTEXAI");
            }
            Some("vertex_adc") | Some("vertex_service_account") | Some("vertex_api_key") => {
                cmd.env("GOOGLE_GENAI_USE_VERTEXAI", "true");
                cmd.env_remove("GOOGLE_GENAI_USE_GCA");
            }
            Some("custom") | Some("gemini_api_key") => {
                cmd.env_remove("GOOGLE_GENAI_USE_GCA");
                cmd.env_remove("GOOGLE_GENAI_USE_VERTEXAI");
            }
            _ => {}
        }
    }

    fn resolve_approval_mode(access_mode: Option<&str>) -> Option<&'static str> {
        let normalized = access_mode
            .map(str::trim)
            .filter(|value| !value.is_empty());
        match normalized {
            Some("full-access") => Some("yolo"),
            Some("read-only") => Some("plan"),
            Some("default") => Some("default"),
            // "current" should respect Gemini CLI's own active/default policy.
            Some("current") | None => None,
            // Keep compatibility for unknown/legacy values.
            Some(_) => Some("auto_edit"),
        }
    }

    fn load_vendor_runtime_config() -> GeminiVendorRuntimeConfig {
        let mut result = GeminiVendorRuntimeConfig::default();
        let Some(home) = dirs::home_dir() else {
            return result;
        };
        let config_path = home.join(".codemoss").join("config.json");
        let Ok(content) = std::fs::read_to_string(config_path) else {
            return result;
        };
        let Ok(root) = serde_json::from_str::<Value>(&content) else {
            return result;
        };
        let Some(gemini) = root.get("gemini").and_then(Value::as_object) else {
            return result;
        };
        let enabled = gemini
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if !enabled {
            return result;
        }
        result.auth_mode = gemini
            .get("auth_mode")
            .or_else(|| gemini.get("authMode"))
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        let Some(env_obj) = gemini.get("env").and_then(Value::as_object) else {
            return result;
        };
        for (key, value) in env_obj {
            let normalized_key = key.trim();
            if normalized_key.is_empty() {
                continue;
            }
            let normalized_value = value.as_str().map(|v| v.trim().to_string()).or_else(|| {
                if value.is_null() {
                    None
                } else {
                    Some(value.to_string())
                }
            });
            let Some(normalized_value) = normalized_value else {
                continue;
            };
            if normalized_value.is_empty() {
                continue;
            }
            result
                .env
                .insert(normalized_key.to_string(), normalized_value);
        }
        result
    }

    fn build_command(&self, params: &SendMessageParams) -> Command {
        let bin = if let Some(ref custom) = self.bin_path {
            custom.clone()
        } else {
            crate::backend::app_server::find_cli_binary("gemini", None)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "gemini".to_string())
        };

        let mut cmd = crate::backend::app_server::build_command_for_binary(&bin);
        cmd.current_dir(&self.workspace_path);
        cmd.arg("--output-format");
        cmd.arg("stream-json");

        if let Some(model) = params
            .model
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            cmd.arg("--model");
            cmd.arg(model);
        }

        if let Some(approval_mode) = Self::resolve_approval_mode(params.access_mode.as_deref()) {
            cmd.arg("--approval-mode");
            cmd.arg(approval_mode);
        }

        if params.continue_session {
            if let Some(session_id) = params
                .session_id
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
            {
                cmd.arg("--resume");
                cmd.arg(session_id);
            }
        }

        if let Some(args) = self.custom_args.as_ref() {
            for arg in args.split_whitespace() {
                cmd.arg(arg);
            }
        }

        let message_text =
            Self::with_external_spec_hint(&params.text, params.custom_spec_root.as_deref());
        let message_text = Self::with_image_references(&message_text, params.images.as_deref());
        let safe_text = if message_text.starts_with('-') {
            format!(" {}", message_text)
        } else {
            message_text
        };
        cmd.arg("--prompt");
        cmd.arg(safe_text);

        let vendor_runtime = Self::load_vendor_runtime_config();
        for (key, value) in vendor_runtime.env {
            cmd.env(key, value);
        }
        Self::apply_auth_mode_env_overrides(&mut cmd, vendor_runtime.auth_mode.as_deref());
        Self::persist_auth_mode_hint(
            vendor_runtime.auth_mode.as_deref(),
            self.home_dir.as_deref(),
        );

        if let Some(home) = self.home_dir.as_ref() {
            cmd.env("GEMINI_CLI_HOME", home);
        }

        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd
    }

    pub async fn send_message(
        &self,
        params: SendMessageParams,
        turn_id: &str,
    ) -> Result<String, String> {
        let mut cmd = self.build_command(&params);
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(error) => {
                let error_msg = format!("Failed to spawn gemini: {}", error);
                self.emit_error(turn_id, error_msg.clone());
                return Err(error_msg);
            }
        };

        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                let error_msg = "Failed to capture stdout".to_string();
                self.emit_error(turn_id, error_msg.clone());
                return Err(error_msg);
            }
        };
        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                let error_msg = "Failed to capture stderr".to_string();
                self.emit_error(turn_id, error_msg.clone());
                return Err(error_msg);
            }
        };

        {
            let mut active = self.active_processes.lock().await;
            active.insert(turn_id.to_string(), child);
        }

        self.emit_turn_event(
            turn_id,
            EngineEvent::SessionStarted {
                workspace_id: self.workspace_id.clone(),
                session_id: "pending".to_string(),
                engine: EngineType::Gemini,
            },
        );
        self.emit_turn_event(
            turn_id,
            EngineEvent::TurnStarted {
                workspace_id: self.workspace_id.clone(),
                turn_id: turn_id.to_string(),
            },
        );

        let stderr_reader = BufReader::new(stderr);
        let stderr_task = tokio::spawn(async move {
            let mut lines = stderr_reader.lines();
            let mut text = String::new();
            while let Ok(Some(line)) = lines.next_line().await {
                text.push_str(&line);
                text.push('\n');
            }
            text
        });

        let mut response_text = String::new();
        let mut saw_turn_completed = false;
        let mut saw_turn_error = false;
        let mut saw_tool_activity = false;
        let mut error_output = String::new();
        let mut session_started_emitted = false;
        let mut new_session_id: Option<String> = None;
        let mut observed_event_types = BTreeSet::new();
        let mut last_reasoning_snapshot = String::new();
        let mut saw_reasoning_output = false;

        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(event) => {
                    if let Some(event_type) = event.get("type").and_then(|value| value.as_str()) {
                        observed_event_types.insert(event_type.to_string());
                    }
                    if let Some(session_id) = extract_session_id(&event) {
                        if !session_started_emitted {
                            session_started_emitted = true;
                            new_session_id = Some(session_id.clone());
                            self.emit_turn_event(
                                turn_id,
                                EngineEvent::SessionStarted {
                                    workspace_id: self.workspace_id.clone(),
                                    session_id,
                                    engine: EngineType::Gemini,
                                },
                            );
                        }
                    }
                    let parsed_event = parse_gemini_event(&self.workspace_id, &event);
                    let parsed_is_reasoning = parsed_event
                        .as_ref()
                        .is_some_and(|entry| matches!(entry, EngineEvent::ReasoningDelta { .. }));
                    if !parsed_is_reasoning {
                        if let Some(thought_text) = extract_latest_thought_text(&event) {
                            if thought_text != last_reasoning_snapshot {
                                last_reasoning_snapshot = thought_text.clone();
                                saw_reasoning_output = true;
                                self.emit_turn_event(
                                    turn_id,
                                    EngineEvent::ReasoningDelta {
                                        workspace_id: self.workspace_id.clone(),
                                        text: thought_text,
                                    },
                                );
                            }
                        }
                    }
                    if let Some(unified_event) = parsed_event {
                        match &unified_event {
                            EngineEvent::TextDelta { text, .. } => {
                                response_text.push_str(text);
                            }
                            EngineEvent::ReasoningDelta { text, .. } => {
                                saw_reasoning_output = true;
                                last_reasoning_snapshot = text.clone();
                            }
                            EngineEvent::ToolStarted { .. } | EngineEvent::ToolCompleted { .. } => {
                                saw_tool_activity = true;
                            }
                            EngineEvent::TurnError { .. } => {
                                saw_turn_error = true;
                            }
                            EngineEvent::TurnCompleted { result, .. } => {
                                saw_turn_completed = true;
                                if response_text.trim().is_empty() {
                                    if let Some(result_text) = result
                                        .as_ref()
                                        .and_then(|value| extract_text_from_value(value, 0))
                                    {
                                        response_text = result_text;
                                    }
                                }
                            }
                            _ => {}
                        }
                        self.emit_turn_event(turn_id, unified_event);
                    }
                }
                Err(_) => {
                    error_output.push_str(&line);
                    error_output.push('\n');
                }
            }
        }

        if !saw_reasoning_output {
            let fallback_session_id = if new_session_id.is_some() {
                new_session_id.clone()
            } else {
                self.get_session_id().await
            };
            if let Some(session_id) = fallback_session_id {
                if let Ok(history) = load_gemini_session(
                    &self.workspace_path,
                    &session_id,
                    self.home_dir.as_deref(),
                )
                .await
                {
                    let fallback_reasoning = collect_latest_turn_reasoning_texts(&history.messages);
                    for text in fallback_reasoning {
                        if text == last_reasoning_snapshot {
                            continue;
                        }
                        last_reasoning_snapshot = text.clone();
                        self.emit_turn_event(
                            turn_id,
                            EngineEvent::ReasoningDelta {
                                workspace_id: self.workspace_id.clone(),
                                text,
                            },
                        );
                    }
                }
            }
        }

        let mut child = {
            let mut active = self.active_processes.lock().await;
            active.remove(turn_id)
        };
        let status = if let Some(mut process) = child.take() {
            process.wait().await.ok()
        } else {
            None
        };
        let stderr_text = stderr_task.await.unwrap_or_default();
        if !stderr_text.trim().is_empty() {
            error_output.push_str(&stderr_text);
        }

        if let Some(status) = status {
            if !status.success() {
                let error_msg = if self.interrupted.swap(false, Ordering::SeqCst) {
                    "Session stopped.".to_string()
                } else if !error_output.trim().is_empty() {
                    error_output.trim().to_string()
                } else {
                    format!("Gemini exited with status: {}", status)
                };
                self.emit_error(turn_id, error_msg.clone());
                return Err(error_msg);
            }
        } else if self.interrupted.swap(false, Ordering::SeqCst) {
            let error_msg = "Session stopped.".to_string();
            self.emit_error(turn_id, error_msg.clone());
            return Err(error_msg);
        }

        if response_text.trim().is_empty() && !error_output.trim().is_empty() {
            let error_msg = error_output.trim().to_string();
            self.emit_error(turn_id, error_msg.clone());
            return Err(error_msg);
        }

        if response_text.trim().is_empty() && saw_turn_error {
            return Err("Gemini returned an error event.".to_string());
        }

        if response_text.trim().is_empty() {
            let observed = if observed_event_types.is_empty() {
                "none".to_string()
            } else {
                observed_event_types
                    .iter()
                    .cloned()
                    .collect::<Vec<String>>()
                    .join(", ")
            };
            let reason = if saw_turn_completed {
                "Gemini completed but produced no assistant output."
            } else {
                "Gemini exited without a completion event or assistant output."
            };
            let diagnostic = format!("{reason} Observed event types: {observed}.");
            if !saw_tool_activity {
                self.emit_error(turn_id, diagnostic.clone());
                return Err(diagnostic);
            }
        }

        if let Some(session_id) = new_session_id {
            self.set_session_id(Some(session_id)).await;
        }

        if !saw_turn_completed && !saw_turn_error {
            self.emit_turn_event(
                turn_id,
                EngineEvent::TurnCompleted {
                    workspace_id: self.workspace_id.clone(),
                    result: Some(json!({
                        "text": response_text,
                    })),
                },
            );
        }

        Ok(response_text)
    }

    pub async fn interrupt(&self) -> Result<(), String> {
        self.interrupted.store(true, Ordering::SeqCst);
        let mut active = self.active_processes.lock().await;
        for child in active.values_mut() {
            child
                .kill()
                .await
                .map_err(|e| format!("Failed to kill process: {}", e))?;
        }
        active.clear();
        Ok(())
    }
}

fn first_non_empty_str<'a>(candidates: &[Option<&'a str>]) -> Option<&'a str> {
    for value in candidates {
        if let Some(text) = value {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn extract_text_from_value(value: &Value, depth: usize) -> Option<String> {
    if depth > 4 {
        return None;
    }
    if let Some(text) = value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return Some(text.to_string());
    }
    if let Some(array) = value.as_array() {
        let mut merged = String::new();
        for item in array {
            if let Some(text) = extract_text_from_value(item, depth + 1) {
                if !merged.is_empty() {
                    merged.push('\n');
                }
                merged.push_str(&text);
            }
        }
        return if merged.trim().is_empty() {
            None
        } else {
            Some(merged)
        };
    }
    if let Some(object) = value.as_object() {
        let direct = first_non_empty_str(&[
            object.get("delta").and_then(|v| v.as_str()),
            object.get("text").and_then(|v| v.as_str()),
            object.get("message").and_then(|v| v.as_str()),
            object.get("content").and_then(|v| v.as_str()),
        ]);
        if let Some(text) = direct {
            return Some(text.to_string());
        }
        for key in [
            "content", "message", "part", "parts", "result", "output", "response", "data",
            "payload",
        ] {
            if let Some(nested) = object.get(key) {
                if let Some(text) = extract_text_from_value(nested, depth + 1) {
                    return Some(text);
                }
            }
        }
    }
    None
}

fn extract_session_id(event: &Value) -> Option<String> {
    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if event_type != "init" {
        return None;
    }
    let session_id = first_non_empty_str(&[
        event.get("session_id").and_then(|v| v.as_str()),
        event.get("sessionId").and_then(|v| v.as_str()),
    ])?;
    Some(session_id.to_string())
}

fn extract_result_error_message(event: &Value) -> Option<String> {
    if let Some(error) = event.get("error") {
        if let Some(message) = extract_text_from_value(error, 0) {
            return Some(message);
        }
        if let Some(message) = error
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(message.to_string());
        }
    }
    first_non_empty_str(&[event.get("message").and_then(|value| value.as_str())])
        .map(|value| value.to_string())
}

fn extract_thought_entry_text(thought: &Value) -> Option<String> {
    let subject = first_non_empty_str(&[
        thought.get("subject").and_then(|value| value.as_str()),
        thought.get("title").and_then(|value| value.as_str()),
    ]);
    let description = first_non_empty_str(&[
        thought.get("description").and_then(|value| value.as_str()),
        thought.get("detail").and_then(|value| value.as_str()),
        thought.get("text").and_then(|value| value.as_str()),
        thought.get("message").and_then(|value| value.as_str()),
    ]);
    match (subject, description) {
        (Some(sub), Some(desc)) => Some(format!("{}: {}", sub, desc)),
        (Some(sub), None) => Some(sub.to_string()),
        (None, Some(desc)) => Some(desc.to_string()),
        (None, None) => None,
    }
}

fn extract_latest_thought_text_from_value(value: &Value, depth: usize) -> Option<String> {
    if depth > 6 {
        return None;
    }
    if let Some(thoughts) = value.get("thoughts").and_then(|candidate| candidate.as_array()) {
        if let Some(latest) = thoughts.iter().rev().find_map(extract_thought_entry_text) {
            return Some(latest);
        }
    }

    if let Some(text) = value
        .get("thought")
        .and_then(extract_thought_entry_text)
        .or_else(|| value.get("currentThought").and_then(extract_thought_entry_text))
        .or_else(|| value.get("latestThought").and_then(extract_thought_entry_text))
    {
        return Some(text);
    }

    if let Some(array) = value.as_array() {
        for item in array.iter().rev() {
            if let Some(latest) = extract_latest_thought_text_from_value(item, depth + 1) {
                return Some(latest);
            }
        }
        return None;
    }

    let Some(object) = value.as_object() else {
        return None;
    };

    for key in [
        "message", "messages", "item", "items", "content", "data", "payload", "result",
        "response", "event", "turn",
    ] {
        if let Some(nested) = object.get(key) {
            if let Some(latest) = extract_latest_thought_text_from_value(nested, depth + 1) {
                return Some(latest);
            }
        }
    }

    for nested in object.values() {
        if let Some(latest) = extract_latest_thought_text_from_value(nested, depth + 1) {
            return Some(latest);
        }
    }
    None
}

fn extract_latest_thought_text(event: &Value) -> Option<String> {
    extract_latest_thought_text_from_value(event, 0)
}

fn extract_reasoning_event_text(event: &Value) -> Option<String> {
    extract_event_text(event)
        .or_else(|| extract_thought_entry_text(event))
        .or_else(|| event.get("thought").and_then(extract_thought_entry_text))
        .or_else(|| event.get("currentThought").and_then(extract_thought_entry_text))
        .or_else(|| event.get("latestThought").and_then(extract_thought_entry_text))
        .or_else(|| extract_latest_thought_text(event))
}

fn parse_completion_event(workspace_id: &str, event: &Value) -> Option<EngineEvent> {
    let status = event
        .get("status")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    let is_error_status = status
        .as_deref()
        .is_some_and(|value| matches!(value, "error" | "failed" | "cancelled" | "canceled"));
    let has_error_payload = event.get("error").is_some_and(|value| !value.is_null());
    if is_error_status || has_error_payload {
        let message = extract_result_error_message(event).unwrap_or_else(|| {
            if let Some(value) = status.as_deref() {
                format!("Gemini result status: {}", value)
            } else {
                "Gemini returned an error result.".to_string()
            }
        });
        return Some(EngineEvent::TurnError {
            workspace_id: workspace_id.to_string(),
            error: message,
            code: None,
        });
    }

    let result_text = event
        .get("text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| event.get("response").and_then(|value| extract_text_from_value(value, 0)))
        .or_else(|| event.get("result").and_then(|value| extract_text_from_value(value, 0)));
    let result_payload = if let Some(text) = result_text {
        Some(json!({
            "text": text,
            "raw": event,
        }))
    } else {
        Some(event.clone())
    };
    Some(EngineEvent::TurnCompleted {
        workspace_id: workspace_id.to_string(),
        result: result_payload,
    })
}

fn collect_latest_turn_reasoning_texts(messages: &[GeminiSessionMessage]) -> Vec<String> {
    let mut collected_reversed: Vec<String> = Vec::new();
    for message in messages.iter().rev() {
        if message.role.eq_ignore_ascii_case("user") {
            break;
        }
        if !message.kind.eq_ignore_ascii_case("reasoning") {
            continue;
        }
        let trimmed = message.text.trim();
        if trimmed.is_empty() {
            continue;
        }
        collected_reversed.push(trimmed.to_string());
    }
    collected_reversed.reverse();
    collected_reversed
}

fn extract_event_text(event: &Value) -> Option<String> {
    first_non_empty_str(&[
        event.get("delta").and_then(|v| v.as_str()),
        event.get("text").and_then(|v| v.as_str()),
        event.get("message").and_then(|v| v.as_str()),
    ])
    .map(|s| s.to_string())
    .or_else(|| {
        event
            .get("content")
            .and_then(|value| extract_text_from_value(value, 0))
    })
    .or_else(|| extract_text_from_value(event, 0))
    .filter(|value| !value.trim().is_empty())
}

fn contains_reasoning_keyword(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    normalized.contains("reason")
        || normalized.contains("think")
        || normalized.contains("thought")
}

fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_i64().is_some_and(|n| n != 0),
        Some(Value::String(raw)) => {
            let normalized = raw.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        }
        _ => false,
    }
}

fn should_treat_message_as_reasoning(event: &Value, role: &str) -> bool {
    if contains_reasoning_keyword(role) {
        return true;
    }
    let kind = event
        .get("kind")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if contains_reasoning_keyword(kind) {
        return true;
    }
    let channel = event
        .get("channel")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if contains_reasoning_keyword(channel) {
        return true;
    }
    is_truthy(event.get("isThought").or_else(|| event.get("is_thought")))
        || is_truthy(event.get("isReasoning").or_else(|| event.get("is_reasoning")))
}

fn is_reasoning_event_type(event_type: &str) -> bool {
    let normalized = event_type.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    matches!(
        normalized.as_str(),
        "reasoning" | "reasoning_delta" | "thinking" | "thinking_delta" | "thought" | "thought_delta"
    ) || contains_reasoning_keyword(&normalized)
}

fn is_text_like_event_type(event_type: &str) -> bool {
    let normalized = event_type.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    matches!(
        normalized.as_str(),
        "text"
            | "content_delta"
            | "text_delta"
            | "output_text_delta"
            | "assistant_message_delta"
            | "message_delta"
            | "assistant_message"
    ) || normalized.contains("message")
        || normalized.contains("text")
}

fn is_completion_event_type(event_type: &str) -> bool {
    let normalized = event_type.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    matches!(
        normalized.as_str(),
        "result"
            | "done"
            | "complete"
            | "completed"
            | "final"
            | "turn_completed"
            | "turn.complete"
            | "response_complete"
            | "response.completed"
    )
}

fn parse_gemini_event(workspace_id: &str, event: &Value) -> Option<EngineEvent> {
    let event_type = event.get("type").and_then(|v| v.as_str())?;
    match event_type {
        "text"
        | "content_delta"
        | "text_delta"
        | "output_text_delta"
        | "assistant_message_delta"
        | "message_delta"
        | "assistant_message" => {
            let text = extract_event_text(event)?;
            Some(EngineEvent::TextDelta {
                workspace_id: workspace_id.to_string(),
                text,
            })
        }
        "reasoning" | "reasoning_delta" | "thinking" | "thinking_delta" | "thought" | "thought_delta" => {
            let text = extract_reasoning_event_text(event)?;
            Some(EngineEvent::ReasoningDelta {
                workspace_id: workspace_id.to_string(),
                text,
            })
        }
        "message" => {
            let role = event
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if role == "user" || role == "system" {
                return None;
            }
            if should_treat_message_as_reasoning(event, &role) {
                let text = extract_reasoning_event_text(event)?;
                return Some(EngineEvent::ReasoningDelta {
                    workspace_id: workspace_id.to_string(),
                    text,
                });
            }
            let text = extract_event_text(event)?;
            Some(EngineEvent::TextDelta {
                workspace_id: workspace_id.to_string(),
                text,
            })
        }
        "tool_use" => {
            let tool_id = first_non_empty_str(&[
                event.get("tool_id").and_then(|v| v.as_str()),
                event.get("toolId").and_then(|v| v.as_str()),
                event.get("id").and_then(|v| v.as_str()),
            ])?
            .to_string();
            let tool_name = first_non_empty_str(&[
                event.get("tool_name").and_then(|v| v.as_str()),
                event.get("toolName").and_then(|v| v.as_str()),
                event.get("name").and_then(|v| v.as_str()),
            ])
            .unwrap_or("tool")
            .to_string();
            let input = event
                .get("parameters")
                .cloned()
                .or_else(|| event.get("args").cloned())
                .or_else(|| event.get("input").cloned());
            Some(EngineEvent::ToolStarted {
                workspace_id: workspace_id.to_string(),
                tool_id,
                tool_name,
                input,
            })
        }
        "tool_result" => {
            let tool_id = first_non_empty_str(&[
                event.get("tool_id").and_then(|v| v.as_str()),
                event.get("toolId").and_then(|v| v.as_str()),
                event.get("id").and_then(|v| v.as_str()),
            ])?
            .to_string();
            let status = event
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let error = first_non_empty_str(&[
                event.get("error").and_then(|v| v.as_str()),
                event.get("message").and_then(|v| v.as_str()),
            ])
            .map(|s| s.to_string())
            .or_else(|| {
                if status.contains("fail") || status.contains("error") {
                    Some("Tool execution failed".to_string())
                } else {
                    None
                }
            });
            let output = event
                .get("output")
                .cloned()
                .or_else(|| event.get("result").cloned())
                .or_else(|| event.get("response").cloned());
            Some(EngineEvent::ToolCompleted {
                workspace_id: workspace_id.to_string(),
                tool_id,
                tool_name: event
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                output,
                error,
            })
        }
        "error" => {
            let message = first_non_empty_str(&[
                event.get("error").and_then(|v| v.as_str()),
                event.get("message").and_then(|v| v.as_str()),
            ])
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(event).unwrap_or_default());
            Some(EngineEvent::TurnError {
                workspace_id: workspace_id.to_string(),
                error: message,
                code: None,
            })
        }
        "result" => parse_completion_event(workspace_id, event),
        _ => {
            if is_completion_event_type(event_type) {
                return parse_completion_event(workspace_id, event);
            }
            if is_reasoning_event_type(event_type) {
                let text = extract_reasoning_event_text(event)?;
                return Some(EngineEvent::ReasoningDelta {
                    workspace_id: workspace_id.to_string(),
                    text,
                });
            }
            if is_text_like_event_type(event_type) {
                let text = extract_event_text(event)?;
                return Some(EngineEvent::TextDelta {
                    workspace_id: workspace_id.to_string(),
                    text,
                });
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        collect_latest_turn_reasoning_texts, extract_latest_thought_text, parse_gemini_event,
        GeminiSession, GeminiSessionMessage,
    };
    use serde_json::json;
    use super::EngineEvent;

    #[test]
    fn selected_auth_type_for_api_key_modes() {
        assert_eq!(
            GeminiSession::selected_auth_type_for_mode(Some("custom")),
            "gemini-api-key"
        );
        assert_eq!(
            GeminiSession::selected_auth_type_for_mode(Some("gemini_api_key")),
            "gemini-api-key"
        );
    }

    #[test]
    fn selected_auth_type_for_vertex_modes() {
        assert_eq!(
            GeminiSession::selected_auth_type_for_mode(Some("vertex_api_key")),
            "vertex-ai"
        );
        assert_eq!(
            GeminiSession::selected_auth_type_for_mode(Some("vertex_adc")),
            "vertex-ai"
        );
        assert_eq!(
            GeminiSession::selected_auth_type_for_mode(Some("vertex_service_account")),
            "vertex-ai"
        );
    }

    #[test]
    fn selected_auth_type_for_login_google_mode() {
        assert_eq!(
            GeminiSession::selected_auth_type_for_mode(Some("login_google")),
            "oauth-personal"
        );
        assert_eq!(
            GeminiSession::selected_auth_type_for_mode(Some("unknown")),
            "oauth-personal"
        );
    }

    #[test]
    fn with_image_references_appends_deduped_at_paths() {
        let images = vec![
            "/tmp/screen 1.png".to_string(),
            "/tmp/screen 1.png".to_string(),
            "/tmp/screen-2.jpg".to_string(),
        ];
        let prompt = GeminiSession::with_image_references("Describe screenshots", Some(images.as_slice()));
        assert_eq!(
            prompt,
            "Describe screenshots\n\n@\"/tmp/screen 1.png\" @\"/tmp/screen-2.jpg\""
        );
    }

    #[test]
    fn with_image_references_strips_file_uri_prefix() {
        let images = vec!["file:///Users/demo/a.png".to_string()];
        let prompt = GeminiSession::with_image_references("Describe", Some(images.as_slice()));
        assert_eq!(prompt, "Describe\n\n@\"/Users/demo/a.png\"");
    }

    #[test]
    fn with_image_references_skips_unsupported_data_urls() {
        let images = vec!["data:image/png;base64,AAAA".to_string()];
        let prompt = GeminiSession::with_image_references("Describe", Some(images.as_slice()));
        assert_eq!(prompt, "Describe");
    }

    #[test]
    fn parse_result_error_maps_to_turn_error() {
        let payload = json!({
            "type": "result",
            "status": "error",
            "error": {
                "message": "quota exceeded"
            }
        });
        let parsed = parse_gemini_event("workspace-1", &payload);
        match parsed {
            Some(EngineEvent::TurnError { error, .. }) => {
                assert!(error.contains("quota exceeded"));
            }
            _ => panic!("expected TurnError"),
        }
    }

    #[test]
    fn parse_result_success_maps_to_turn_completed() {
        let payload = json!({
            "type": "result",
            "status": "success",
            "text": "你好"
        });
        let parsed = parse_gemini_event("workspace-1", &payload);
        assert!(matches!(parsed, Some(EngineEvent::TurnCompleted { .. })));
    }

    #[test]
    fn parse_reasoning_delta_alias_maps_to_reasoning_delta() {
        let payload = json!({
            "type": "reasoning_delta",
            "delta": "先规划，再执行"
        });
        let parsed = parse_gemini_event("workspace-1", &payload);
        match parsed {
            Some(EngineEvent::ReasoningDelta { text, .. }) => {
                assert_eq!(text, "先规划，再执行");
            }
            _ => panic!("expected ReasoningDelta"),
        }
    }

    #[test]
    fn parse_thought_event_with_subject_description_maps_to_reasoning_delta() {
        let payload = json!({
            "type": "thought",
            "subject": "读取项目结构",
            "description": "先检查 README 和 pom.xml"
        });
        let parsed = parse_gemini_event("workspace-1", &payload);
        match parsed {
            Some(EngineEvent::ReasoningDelta { text, .. }) => {
                assert_eq!(text, "读取项目结构: 先检查 README 和 pom.xml");
            }
            _ => panic!("expected ReasoningDelta"),
        }
    }

    #[test]
    fn parse_reasoning_keyword_event_with_nested_thought_maps_to_reasoning_delta() {
        let payload = json!({
            "type": "assistant_thinking_update",
            "thought": {
                "subject": "规划步骤",
                "description": "先看配置再看源码"
            }
        });
        let parsed = parse_gemini_event("workspace-1", &payload);
        match parsed {
            Some(EngineEvent::ReasoningDelta { text, .. }) => {
                assert_eq!(text, "规划步骤: 先看配置再看源码");
            }
            _ => panic!("expected ReasoningDelta"),
        }
    }

    #[test]
    fn parse_reasoning_keyword_event_with_nested_message_thoughts_maps_to_reasoning_delta() {
        let payload = json!({
            "type": "assistant_thinking_update",
            "message": {
                "thoughts": [
                    {
                        "subject": "读取项目结构",
                        "description": "先看 README 和 package.json"
                    }
                ]
            }
        });
        let parsed = parse_gemini_event("workspace-1", &payload);
        match parsed {
            Some(EngineEvent::ReasoningDelta { text, .. }) => {
                assert_eq!(text, "读取项目结构: 先看 README 和 package.json");
            }
            _ => panic!("expected ReasoningDelta"),
        }
    }

    #[test]
    fn parse_message_with_reasoning_role_maps_to_reasoning_delta() {
        let payload = json!({
            "type": "message",
            "role": "assistant_reasoning",
            "delta": "分析上下文..."
        });
        let parsed = parse_gemini_event("workspace-1", &payload);
        match parsed {
            Some(EngineEvent::ReasoningDelta { text, .. }) => {
                assert_eq!(text, "分析上下文...");
            }
            _ => panic!("expected ReasoningDelta"),
        }
    }

    #[test]
    fn parse_message_delta_alias_maps_to_text_delta() {
        let payload = json!({
            "type": "message_delta",
            "delta": "回复片段"
        });
        let parsed = parse_gemini_event("workspace-1", &payload);
        match parsed {
            Some(EngineEvent::TextDelta { text, .. }) => {
                assert_eq!(text, "回复片段");
            }
            _ => panic!("expected TextDelta"),
        }
    }

    #[test]
    fn parse_done_alias_maps_to_turn_completed() {
        let payload = json!({
            "type": "done",
            "status": "success",
            "text": "完成"
        });
        let parsed = parse_gemini_event("workspace-1", &payload);
        assert!(matches!(parsed, Some(EngineEvent::TurnCompleted { .. })));
    }

    #[test]
    fn extract_latest_thought_text_prefers_latest_non_empty_entry() {
        let payload = json!({
            "thoughts": [
                {
                    "subject": "先检查上下文",
                    "description": "确认用户意图"
                },
                {
                    "subject": "再输出答案",
                    "description": "整理最终结论"
                }
            ]
        });
        let extracted = extract_latest_thought_text(&payload);
        assert_eq!(extracted.as_deref(), Some("再输出答案: 整理最终结论"));
    }

    #[test]
    fn extract_latest_thought_text_reads_nested_message_payload() {
        let payload = json!({
            "type": "message",
            "message": {
                "messages": [
                    {
                        "type": "assistant",
                        "thoughts": [
                            {
                                "subject": "先收集上下文",
                                "description": "读取 docs 和 src 目录"
                            },
                            {
                                "subject": "再生成结论",
                                "description": "整理关键变更点"
                            }
                        ]
                    }
                ]
            }
        });
        let extracted = extract_latest_thought_text(&payload);
        assert_eq!(extracted.as_deref(), Some("再生成结论: 整理关键变更点"));
    }

    #[test]
    fn approval_mode_current_uses_cli_default() {
        assert_eq!(GeminiSession::resolve_approval_mode(Some("current")), None);
    }

    #[test]
    fn approval_mode_full_access_maps_to_yolo() {
        assert_eq!(
            GeminiSession::resolve_approval_mode(Some("full-access")),
            Some("yolo")
        );
    }

    #[test]
    fn collect_latest_turn_reasoning_texts_stops_at_latest_user_boundary() {
        let messages = vec![
            GeminiSessionMessage {
                id: "old-r1".to_string(),
                role: "assistant".to_string(),
                text: "旧思考".to_string(),
                images: None,
                timestamp: None,
                kind: "reasoning".to_string(),
                tool_type: None,
                title: None,
                tool_input: None,
                tool_output: None,
            },
            GeminiSessionMessage {
                id: "old-a1".to_string(),
                role: "assistant".to_string(),
                text: "旧正文".to_string(),
                images: None,
                timestamp: None,
                kind: "message".to_string(),
                tool_type: None,
                title: None,
                tool_input: None,
                tool_output: None,
            },
            GeminiSessionMessage {
                id: "u-last".to_string(),
                role: "user".to_string(),
                text: "新的提问".to_string(),
                images: None,
                timestamp: None,
                kind: "message".to_string(),
                tool_type: None,
                title: None,
                tool_input: None,
                tool_output: None,
            },
            GeminiSessionMessage {
                id: "r-last-1".to_string(),
                role: "assistant".to_string(),
                text: "先看目录".to_string(),
                images: None,
                timestamp: None,
                kind: "reasoning".to_string(),
                tool_type: None,
                title: None,
                tool_input: None,
                tool_output: None,
            },
            GeminiSessionMessage {
                id: "r-last-2".to_string(),
                role: "assistant".to_string(),
                text: "再读 README".to_string(),
                images: None,
                timestamp: None,
                kind: "reasoning".to_string(),
                tool_type: None,
                title: None,
                tool_input: None,
                tool_output: None,
            },
        ];
        let collected = collect_latest_turn_reasoning_texts(&messages);
        assert_eq!(collected, vec!["先看目录".to_string(), "再读 README".to_string()]);
    }
}
