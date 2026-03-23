use super::*;

impl ClaudeSession {
    pub(crate) fn is_prompt_too_long_error(error: &str) -> bool {
        let lower = error.to_ascii_lowercase();
        lower.contains("prompt is too long")
            || lower.contains("prompt too long")
            || lower.contains("maximum context length")
            || lower.contains("max context length")
            || lower.contains("context length exceeded")
            || lower.contains("token limit exceeded")
    }

    pub(crate) fn mark_retryable_prompt_too_long_error(error: &str) -> String {
        if error.starts_with(RETRYABLE_PROMPT_TOO_LONG_PREFIX) {
            return error.to_string();
        }
        format!("{RETRYABLE_PROMPT_TOO_LONG_PREFIX}{error}")
    }

    pub(crate) fn extract_retryable_prompt_too_long_error(error: &str) -> Option<String> {
        error
            .strip_prefix(RETRYABLE_PROMPT_TOO_LONG_PREFIX)
            .map(|value| value.to_string())
    }

    pub(crate) fn clear_retryable_prompt_too_long_marker(error: String) -> String {
        Self::extract_retryable_prompt_too_long_error(&error).unwrap_or(error)
    }

    fn normalize_compaction_signal_from_text(value: &str) -> Option<&'static str> {
        let normalized = value.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        if normalized.is_empty() {
            return None;
        }
        if normalized.contains("compaction_failed")
            || normalized.contains("compact_failed")
            || normalized.contains("compactfailure")
        {
            return Some("compaction_failed");
        }
        if normalized.contains("compact_boundary") || normalized.contains("compacted") {
            return Some("compact_boundary");
        }
        if normalized.contains("compacting") {
            return Some("compacting");
        }
        None
    }

    pub(super) fn has_compaction_system_signal(event: &Value) -> bool {
        for key in [
            "subtype",
            "subType",
            "event",
            "event_type",
            "eventType",
            "name",
            "kind",
            "status",
            "phase",
            "state",
            "type",
        ] {
            if let Some(raw) = event.get(key).and_then(|value| value.as_str()) {
                if Self::normalize_compaction_signal_from_text(raw).is_some() {
                    return true;
                }
            }
        }
        false
    }

    fn emit_compaction_signal(
        &self,
        turn_id: &str,
        subtype: &str,
        extra_fields: Option<serde_json::Map<String, Value>>,
    ) {
        let mut payload = serde_json::Map::new();
        payload.insert("type".to_string(), Value::String("system".to_string()));
        payload.insert("subtype".to_string(), Value::String(subtype.to_string()));
        payload.insert(
            "source".to_string(),
            Value::String(AUTO_COMPACT_SIGNAL_SOURCE.to_string()),
        );
        if let Some(extra) = extra_fields {
            for (key, value) in extra {
                payload.insert(key, value);
            }
        }
        self.emit_turn_event(
            turn_id,
            EngineEvent::Raw {
                workspace_id: self.workspace_id.clone(),
                engine: EngineType::Claude,
                data: Value::Object(payload),
            },
        );
    }

    pub async fn send_message_with_auto_compact_retry(
        &self,
        params: SendMessageParams,
        turn_id: &str,
    ) -> Result<String, String> {
        let first_attempt = self.send_message(params.clone(), turn_id).await;
        let first_error = match first_attempt {
            Ok(response) => return Ok(response),
            Err(error) => error,
        };

        let trigger_error = match Self::extract_retryable_prompt_too_long_error(&first_error) {
            Some(error) => error,
            None => return Err(Self::clear_retryable_prompt_too_long_marker(first_error)),
        };

        log::warn!(
            "[claude] turn={} hit prompt-too-long boundary, triggering one-time /compact recovery",
            turn_id
        );

        self.emit_compaction_signal(turn_id, "compacting", None);

        let mut compact_params = params.clone();
        compact_params.text = "/compact".to_string();
        compact_params.images = None;
        compact_params.continue_session = true;
        if compact_params.session_id.is_none() {
            compact_params.session_id = self.get_session_id().await;
        }
        let compact_turn_id = format!("{turn_id}::auto-compact");
        if let Err(compact_error) = self.send_message(compact_params, &compact_turn_id).await {
            let compact_error = Self::clear_retryable_prompt_too_long_marker(compact_error);
            let failure_message = format!(
                "Prompt is too long and automatic /compact failed: {}",
                compact_error
            );
            let mut failure_payload = serde_json::Map::new();
            failure_payload.insert("reason".to_string(), Value::String(failure_message.clone()));
            self.emit_compaction_signal(turn_id, "compaction_failed", Some(failure_payload));
            self.emit_error(turn_id, failure_message.clone());
            return Err(failure_message);
        }

        let mut retry_params = params;
        retry_params.continue_session = true;
        if retry_params.session_id.is_none() {
            retry_params.session_id = self.get_session_id().await;
        }
        match self.send_message(retry_params, turn_id).await {
            Ok(response) => Ok(response),
            Err(retry_error) => {
                let retry_error = Self::clear_retryable_prompt_too_long_marker(retry_error);
                let final_message = format!(
                    "Prompt is too long. Retried once after /compact but still failed: {}",
                    retry_error
                );
                log::error!(
                    "[claude] auto /compact retry failed (turn={}): trigger={}, final={}",
                    turn_id,
                    trigger_error,
                    final_message
                );
                self.emit_error(turn_id, final_message.clone());
                Err(final_message)
            }
        }
    }

    /// Try to extract context window usage from any event
    /// Claude CLI may provide usage data in multiple locations:
    /// 1. context_window.current_usage (statusline/hooks - most accurate)
    /// 2. message.usage (assistant events)
    /// 3. usage (top-level usage field)
    pub(super) fn try_extract_context_window_usage(&self, turn_id: &str, event: &Value) {
        let (usage, model_context_window) = self.find_usage_data(event);

        if let Some(usage) = usage {
            let input_tokens = usage
                .get("input_tokens")
                .or_else(|| usage.get("inputTokens"))
                .and_then(|v| v.as_i64());

            let output_tokens = usage
                .get("output_tokens")
                .or_else(|| usage.get("outputTokens"))
                .and_then(|v| v.as_i64());

            let cache_creation = usage
                .get("cache_creation_input_tokens")
                .or_else(|| usage.get("cacheCreationInputTokens"))
                .or_else(|| usage.get("cache_creation_tokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let cache_read = usage
                .get("cache_read_input_tokens")
                .or_else(|| usage.get("cacheReadInputTokens"))
                .or_else(|| usage.get("cache_read_tokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let cached_tokens = if cache_creation > 0 || cache_read > 0 {
                Some(cache_creation + cache_read)
            } else {
                None
            };

            if input_tokens.is_some() {
                log::debug!(
                    "[claude] Emitting UsageUpdate: input={:?}, output={:?}, cached={:?}, window={:?}",
                    input_tokens, output_tokens, cached_tokens, model_context_window
                );
                self.emit_turn_event(
                    turn_id,
                    EngineEvent::UsageUpdate {
                        workspace_id: self.workspace_id.clone(),
                        input_tokens,
                        output_tokens,
                        cached_tokens,
                        model_context_window,
                    },
                );
            }
        }
    }

    /// Find usage data from various locations in the event
    /// Returns (usage_data, model_context_window)
    fn find_usage_data<'a>(&self, event: &'a Value) -> (Option<&'a Value>, Option<i64>) {
        if let Some(context_window) = event.get("context_window") {
            log::debug!(
                "[claude] Found context_window field: {}",
                serde_json::to_string_pretty(context_window)
                    .unwrap_or_else(|_| context_window.to_string())
            );

            let model_context_window = context_window
                .get("context_window_size")
                .or_else(|| context_window.get("contextWindowSize"))
                .and_then(|v| v.as_i64());

            if let Some(current_usage) = context_window
                .get("current_usage")
                .or_else(|| context_window.get("currentUsage"))
            {
                return (Some(current_usage), model_context_window);
            }
        }

        if let Some(message) = event.get("message") {
            if let Some(usage) = message.get("usage") {
                log::debug!(
                    "[claude] Found message.usage field: {}",
                    serde_json::to_string_pretty(usage).unwrap_or_else(|_| usage.to_string())
                );
                return (Some(usage), None);
            }
        }

        if let Some(usage) = event.get("usage") {
            log::debug!(
                "[claude] Found top-level usage field: {}",
                serde_json::to_string_pretty(usage).unwrap_or_else(|_| usage.to_string())
            );
            return (Some(usage), None);
        }

        log::debug!(
            "[claude] No usage data found in event type: {:?}",
            event.get("type").and_then(|v| v.as_str())
        );
        (None, None)
    }
}
