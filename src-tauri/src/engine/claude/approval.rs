use super::*;
use std::path::{Component, Path, PathBuf};

pub(super) const SYNTHETIC_APPROVAL_RESUME_MARKER_PREFIX: &str = "<ccgui-approval-resume>";
pub(super) const SYNTHETIC_APPROVAL_RESUME_MARKER_SUFFIX: &str = "</ccgui-approval-resume>";
const MAX_CLAUDE_APPROVAL_FILE_BYTES: usize = 400_000;
const CLAUDE_FILE_PATH_KEYS: &[&str] = &[
    "file_path",
    "filePath",
    "filepath",
    "path",
    "target_file",
    "targetFile",
    "filename",
    "file",
    "notebook_path",
    "notebookPath",
];
const CLAUDE_FILE_CONTENT_KEYS: &[&str] = &["content", "text", "new_string", "newString"];

#[derive(Clone, Copy)]
pub(super) enum ClaudeModeBlockedKind {
    RequestUserInput,
    FileChange,
    CommandExecution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalClaudeFileApprovalAction {
    Write {
        relative_path: String,
        content: String,
    },
    CreateDirectory {
        relative_path: String,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyntheticApprovalSummaryEntry {
    pub(super) summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) kind: Option<String>,
    pub(super) status: String,
}

pub(super) fn format_synthetic_approval_completion_text(
    entries: &[SyntheticApprovalSummaryEntry],
) -> Option<String> {
    let filtered: Vec<&str> = entries
        .iter()
        .map(|entry| entry.summary.as_str())
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect();
    match filtered.len() {
        0 => None,
        1 => Some(filtered[0].to_string()),
        _ => Some(format!(
            "Completed approved operations:\n{}",
            filtered
                .iter()
                .map(|entry| format!("- {entry}"))
                .collect::<Vec<String>>()
                .join("\n")
        )),
    }
}

fn format_synthetic_approval_resume_marker(
    entries: &[SyntheticApprovalSummaryEntry],
) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    serde_json::to_string(entries).ok().map(|payload| {
        format!(
            "{SYNTHETIC_APPROVAL_RESUME_MARKER_PREFIX}{payload}{SYNTHETIC_APPROVAL_RESUME_MARKER_SUFFIX}"
        )
    })
}

pub(super) fn format_synthetic_approval_resume_message(
    entries: &[SyntheticApprovalSummaryEntry],
) -> String {
    let summary = format_synthetic_approval_completion_text(entries)
        .unwrap_or_else(|| "Approval handling finished.".to_string());
    match format_synthetic_approval_resume_marker(entries) {
        Some(marker) => format!(
            "{marker}\n{summary}\nPlease continue from the current workspace state and finish the original task."
        ),
        None => format!(
            "{}\nPlease continue from the current workspace state and finish the original task.",
            summary
        ),
    }
}

pub(super) fn classify_claude_mode_blocked_tool(tool_name: &str) -> Option<ClaudeModeBlockedKind> {
    let normalized = tool_name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized == "askuserquestion" {
        return Some(ClaudeModeBlockedKind::RequestUserInput);
    }
    if normalized.contains("bash")
        || normalized.contains("exec")
        || normalized.contains("command")
        || normalized.contains("shell")
        || normalized.contains("terminal")
        || normalized.contains("stdin")
    {
        return Some(ClaudeModeBlockedKind::CommandExecution);
    }
    if normalized == "edit"
        || normalized == "multiedit"
        || normalized == "write"
        || normalized == "rewrite"
        || normalized == "createfile"
        || normalized == "createdirectory"
        || normalized == "notebookedit"
    {
        return Some(ClaudeModeBlockedKind::FileChange);
    }
    None
}

pub(super) fn normalize_claude_request_id_key(request_id: &Value) -> Option<String> {
    match request_id {
        Value::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn normalize_claude_tool_name_for_local_apply(tool_name: &str) -> String {
    tool_name
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-', ' '], "")
}

fn extract_first_non_empty_string(value: &Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|candidate| !candidate.is_empty())
            .map(ToString::to_string)
    })
}

fn normalize_claude_raw_path(raw_path: &str) -> String {
    raw_path.trim().replace('\\', "/")
}

pub(super) fn normalize_claude_workspace_relative_path(path: &Path) -> Result<String, String> {
    let mut segments = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => {
                let value = segment.to_string_lossy().trim().to_string();
                if value.is_empty() {
                    return Err("Claude approval path is invalid.".to_string());
                }
                segments.push(value);
            }
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err("Claude approval path is invalid.".to_string());
            }
        }
    }

    if segments.is_empty() {
        return Err("Claude approval path is empty.".to_string());
    }

    let normalized = segments.join("/");
    if normalized == ".git"
        || normalized.starts_with(".git/")
        || normalized.contains("/.git/")
        || normalized.ends_with("/.git")
    {
        return Err("Cannot write inside .git directory.".to_string());
    }

    Ok(normalized)
}

fn ensure_workspace_path_within_root(
    candidate: &Path,
    canonical_root: &Path,
) -> Result<(), String> {
    if !candidate.starts_with(canonical_root) {
        return Err("Invalid path outside workspace root.".to_string());
    }
    Ok(())
}

fn write_claude_approved_workspace_file(
    workspace_root: &Path,
    relative_path: &str,
    content: &str,
) -> Result<(), String> {
    let canonical_root = workspace_root
        .canonicalize()
        .map_err(|error| format!("Failed to resolve workspace root: {error}"))?;
    let candidate = canonical_root.join(relative_path);
    ensure_workspace_path_within_root(&candidate, &canonical_root)?;

    if content.len() > MAX_CLAUDE_APPROVAL_FILE_BYTES {
        return Err("File content exceeds maximum allowed size".to_string());
    }

    if let Some(parent) = candidate.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create parent directories: {error}"))?;
        let canonical_parent = parent
            .canonicalize()
            .map_err(|error| format!("Failed to resolve parent directory: {error}"))?;
        ensure_workspace_path_within_root(&canonical_parent, &canonical_root)?;
    }

    std::fs::write(&candidate, content).map_err(|error| format!("Failed to write file: {error}"))
}

fn create_claude_approved_workspace_directory(
    workspace_root: &Path,
    relative_path: &str,
) -> Result<(), String> {
    let canonical_root = workspace_root
        .canonicalize()
        .map_err(|error| format!("Failed to resolve workspace root: {error}"))?;
    let candidate = canonical_root.join(relative_path);
    ensure_workspace_path_within_root(&candidate, &canonical_root)?;

    if candidate.exists() {
        let metadata = std::fs::metadata(&candidate)
            .map_err(|error| format!("Failed to read path metadata: {error}"))?;
        if metadata.is_dir() {
            return Ok(());
        }
        return Err("Path already exists and is not a directory.".to_string());
    }

    std::fs::create_dir_all(&candidate)
        .map_err(|error| format!("Failed to create directory: {error}"))?;
    let canonical_dir = candidate
        .canonicalize()
        .map_err(|error| format!("Failed to resolve created directory: {error}"))?;
    ensure_workspace_path_within_root(&canonical_dir, &canonical_root)
}

impl ClaudeSession {
    pub(super) fn push_synthetic_approval_summary(
        &self,
        turn_id: &str,
        entry: SyntheticApprovalSummaryEntry,
    ) {
        let trimmed = entry.summary.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Ok(mut entries) = self.synthetic_approval_summaries_by_turn.lock() {
            entries
                .entry(turn_id.to_string())
                .or_default()
                .push(SyntheticApprovalSummaryEntry {
                    summary: trimmed.to_string(),
                    ..entry
                });
        }
    }

    pub(super) fn take_synthetic_approval_entries(
        &self,
        turn_id: &str,
    ) -> Vec<SyntheticApprovalSummaryEntry> {
        self.synthetic_approval_summaries_by_turn
            .lock()
            .ok()
            .and_then(|mut entries| entries.remove(turn_id))
            .unwrap_or_default()
    }

    fn summarize_non_file_approval(
        &self,
        summary: String,
        accepted: bool,
    ) -> SyntheticApprovalSummaryEntry {
        SyntheticApprovalSummaryEntry {
            summary,
            path: None,
            kind: None,
            status: if accepted {
                "completed".to_string()
            } else {
                "failed".to_string()
            },
        }
    }

    pub(super) fn store_approval_resume_message(&self, turn_id: &str, message: String) {
        if let Ok(mut messages) = self.approval_resume_message_by_turn.lock() {
            messages.insert(turn_id.to_string(), message);
        }
        if let Ok(notifies) = self.approval_notify_by_turn.lock() {
            if let Some(notify) = notifies.get(turn_id) {
                notify.notify_waiters();
            }
        }
    }

    pub(super) fn has_approval_resume_waiter_for_turn(&self, turn_id: &str) -> bool {
        self.approval_notify_by_turn
            .lock()
            .ok()
            .map(|notifies| notifies.contains_key(turn_id))
            .unwrap_or(false)
    }

    pub(super) fn finalize_synthetic_approval_turn(&self, turn_id: &str, result: Value) {
        self.emit_turn_event(
            turn_id,
            EngineEvent::TurnCompleted {
                workspace_id: self.workspace_id.clone(),
                result: Some(result),
            },
        );
        self.clear_turn_ephemeral_state(turn_id);
    }

    pub fn has_pending_approval_request(&self, request_id: &Value) -> bool {
        let Some(request_key) = normalize_claude_request_id_key(request_id) else {
            return false;
        };
        self.pending_approval_requests
            .lock()
            .ok()
            .map(|pending| pending.contains_key(&request_key))
            .unwrap_or(false)
    }

    pub(super) fn has_pending_approval_request_for_turn(&self, turn_id: &str) -> bool {
        self.pending_approval_requests
            .lock()
            .ok()
            .map(|pending| pending.values().any(|value| value == turn_id))
            .unwrap_or(false)
    }

    fn pending_approval_request_count_for_turn(&self, turn_id: &str) -> usize {
        self.pending_approval_requests
            .lock()
            .ok()
            .map(|pending| pending.values().filter(|value| *value == turn_id).count())
            .unwrap_or(0)
    }

    fn resolve_workspace_relative_tool_path(&self, raw_path: &str) -> Result<String, String> {
        let normalized_raw_path = normalize_claude_raw_path(raw_path);
        if normalized_raw_path.is_empty() {
            return Err("Claude approval path is empty.".to_string());
        }

        let candidate = PathBuf::from(&normalized_raw_path);
        if candidate.is_absolute() {
            let configured_root = self.workspace_path.clone();
            let canonical_root = configured_root
                .canonicalize()
                .map_err(|error| format!("Failed to resolve workspace root: {error}"))?;
            let relative = candidate
                .strip_prefix(&configured_root)
                .or_else(|_| candidate.strip_prefix(&canonical_root))
                .map_err(|_| {
                    format!(
                        "Claude approval path is outside workspace: {}",
                        candidate.display()
                    )
                })?;
            return normalize_claude_workspace_relative_path(relative);
        }

        normalize_claude_workspace_relative_path(&candidate)
    }

    fn resolve_local_file_approval_action(
        &self,
        tool_id: &str,
    ) -> Result<LocalClaudeFileApprovalAction, String> {
        let tool_name = self.peek_tool_name(tool_id).ok_or_else(|| {
            format!("Missing Claude tool metadata for approval request: {tool_id}")
        })?;
        let input = self
            .peek_tool_input_value(tool_id)
            .ok_or_else(|| format!("Missing Claude tool input for approval request: {tool_id}"))?;
        let raw_path = extract_first_non_empty_string(&input, CLAUDE_FILE_PATH_KEYS)
            .ok_or_else(|| format!("Claude approval tool `{tool_name}` did not include a path."))?;
        let relative_path = self.resolve_workspace_relative_tool_path(&raw_path)?;
        let normalized_tool_name = normalize_claude_tool_name_for_local_apply(&tool_name);

        match normalized_tool_name.as_str() {
            "write" | "writefile" | "createfile" => {
                let content =
                    extract_first_non_empty_string(&input, CLAUDE_FILE_CONTENT_KEYS).unwrap_or_default();
                Ok(LocalClaudeFileApprovalAction::Write {
                    relative_path,
                    content,
                })
            }
            "createdirectory" => Ok(LocalClaudeFileApprovalAction::CreateDirectory {
                relative_path,
            }),
            _ => Err(format!(
                "Claude preview approval currently supports only Write/CreateFile/CreateDirectory. Tool `{tool_name}` is not supported yet."
            )),
        }
    }

    fn apply_local_file_approval(
        &self,
        tool_id: &str,
    ) -> Result<SyntheticApprovalSummaryEntry, String> {
        match self.resolve_local_file_approval_action(tool_id)? {
            LocalClaudeFileApprovalAction::Write {
                relative_path,
                content,
            } => {
                let target_path = self.workspace_path.join(&relative_path);
                let existed_before_write = target_path.exists();
                write_claude_approved_workspace_file(
                    &self.workspace_path,
                    &relative_path,
                    &content,
                )?;
                Ok(SyntheticApprovalSummaryEntry {
                    summary: if existed_before_write {
                        format!("Approved and updated {relative_path}")
                    } else {
                        format!("Approved and wrote {relative_path}")
                    },
                    path: Some(relative_path),
                    kind: Some(
                        if existed_before_write {
                            "modified"
                        } else {
                            "add"
                        }
                        .to_string(),
                    ),
                    status: "completed".to_string(),
                })
            }
            LocalClaudeFileApprovalAction::CreateDirectory { relative_path } => {
                create_claude_approved_workspace_directory(&self.workspace_path, &relative_path)?;
                Ok(SyntheticApprovalSummaryEntry {
                    summary: format!("Approved and created directory {relative_path}"),
                    path: Some(relative_path),
                    kind: Some("add".to_string()),
                    status: "completed".to_string(),
                })
            }
        }
    }

    pub async fn respond_to_approval_request(
        &self,
        request_id: Value,
        result: Value,
    ) -> Result<(), String> {
        let Some(request_key) = normalize_claude_request_id_key(&request_id) else {
            return Err("invalid request_id for Claude approval".to_string());
        };
        let decision = match result {
            Value::String(value) => value.trim().to_ascii_lowercase(),
            Value::Object(map) => map
                .get("decision")
                .and_then(Value::as_str)
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "invalid result for Claude approval".to_string())?,
            _ => return Err("invalid result for Claude approval".to_string()),
        };
        if decision != "accept" && decision != "decline" {
            return Err(format!("unsupported Claude approval result: {decision}"));
        }

        let Some(turn_id) = self
            .pending_approval_requests
            .lock()
            .map_err(|_| "pending_approval_requests lock poisoned".to_string())?
            .remove(&request_key)
        else {
            return Err(format!(
                "unknown request_id for Claude approval: {request_key}"
            ));
        };

        let tool_name = self.peek_tool_name(&request_key).unwrap_or_default();
        let is_file_change = matches!(
            classify_claude_mode_blocked_tool(&tool_name),
            Some(ClaudeModeBlockedKind::FileChange)
        );

        if !is_file_change {
            let summary = if decision == "accept" {
                "Approval acknowledged. The blocked request was not executed automatically; retry after changing Claude mode or permissions.".to_string()
            } else {
                "Blocked request was declined in the approval dialog.".to_string()
            };
            let error = if decision == "decline" {
                Some(summary.clone())
            } else {
                None
            };
            self.emit_tool_completion(&turn_id, &request_key, Some(summary.clone()), error);
            self.push_synthetic_approval_summary(
                &turn_id,
                self.summarize_non_file_approval(summary.clone(), decision == "accept"),
            );
            log::info!(
                "[claude] synthetic approval request acknowledged (request_id={}, result={})",
                request_key,
                decision
            );
            if self.pending_approval_request_count_for_turn(&turn_id) == 0 {
                let approval_entries = self.take_synthetic_approval_entries(&turn_id);
                let aggregated_summary =
                    format_synthetic_approval_completion_text(&approval_entries)
                        .unwrap_or_else(|| summary.trim().to_string());
                if self.has_approval_resume_waiter_for_turn(&turn_id) {
                    let resume_message =
                        format_synthetic_approval_resume_message(&approval_entries);
                    self.store_approval_resume_message(&turn_id, resume_message);
                } else {
                    self.finalize_synthetic_approval_turn(
                        &turn_id,
                        json!({
                            "syntheticApprovalResolved": true,
                            "approved": decision == "accept",
                            "text": aggregated_summary,
                        }),
                    );
                }
            }
            return Ok(());
        }

        if decision == "decline" {
            let message = "File change was declined in the approval dialog.".to_string();
            self.emit_tool_completion(
                &turn_id,
                &request_key,
                Some(message.clone()),
                Some(message.clone()),
            );
            self.push_synthetic_approval_summary(
                &turn_id,
                SyntheticApprovalSummaryEntry {
                    summary: message.clone(),
                    path: None,
                    kind: None,
                    status: "failed".to_string(),
                },
            );
            if self.pending_approval_request_count_for_turn(&turn_id) == 0 {
                let approval_entries = self.take_synthetic_approval_entries(&turn_id);
                let aggregated_summary =
                    format_synthetic_approval_completion_text(&approval_entries)
                        .unwrap_or_else(|| message.trim().to_string());
                if self.has_approval_resume_waiter_for_turn(&turn_id) {
                    let resume_message =
                        format_synthetic_approval_resume_message(&approval_entries);
                    self.store_approval_resume_message(&turn_id, resume_message);
                } else {
                    self.finalize_synthetic_approval_turn(
                        &turn_id,
                        json!({
                            "syntheticApprovalResolved": true,
                            "approved": false,
                            "text": aggregated_summary,
                        }),
                    );
                }
            }
            return Ok(());
        }

        let completion_result = match self.apply_local_file_approval(&request_key) {
            Ok(entry) => {
                self.emit_tool_completion(
                    &turn_id,
                    &request_key,
                    Some(entry.summary.clone()),
                    None,
                );
                self.push_synthetic_approval_summary(&turn_id, entry.clone());
                entry
            }
            Err(error) => {
                log::warn!(
                    "[claude] failed to apply approved file change locally (request_id={}): {}",
                    request_key,
                    error
                );
                self.emit_tool_completion(
                    &turn_id,
                    &request_key,
                    Some(error.clone()),
                    Some(error.clone()),
                );
                let entry = SyntheticApprovalSummaryEntry {
                    summary: error.clone(),
                    path: None,
                    kind: None,
                    status: "failed".to_string(),
                };
                self.push_synthetic_approval_summary(&turn_id, entry.clone());
                entry
            }
        };
        if self.pending_approval_request_count_for_turn(&turn_id) == 0 {
            let approval_entries = self.take_synthetic_approval_entries(&turn_id);
            let aggregated_summary = format_synthetic_approval_completion_text(&approval_entries)
                .unwrap_or_else(|| completion_result.summary.trim().to_string());
            if self.has_approval_resume_waiter_for_turn(&turn_id) {
                let resume_message = format_synthetic_approval_resume_message(&approval_entries);
                self.store_approval_resume_message(&turn_id, resume_message);
            } else {
                self.finalize_synthetic_approval_turn(
                    &turn_id,
                    json!({
                        "syntheticApprovalResolved": true,
                        "approved": decision == "accept",
                        "text": aggregated_summary,
                    }),
                );
            }
        }
        Ok(())
    }
}
