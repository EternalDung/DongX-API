//! OpenAI Chat Completions SSE -> Anthropic Messages SSE.
//!
//! This is the outbound half of the `/v1/messages` bridge. The inbound half
//! (`anthropic_to_chat_request` in `server/handler.rs`) translates an Anthropic
//! request into a Chat request; the shared chat pipeline then runs exactly like
//! `/v1/chat/completions` and yields a Chat SSE stream. This module turns those
//! Chat `chat.completion.chunk` frames into the Anthropic Messages event
//! stream:
//!
//! ```text
//! message_start
//!   content_block_start (thinking)   [only if reasoning_content present]
//!   content_block_delta (thinking_delta)
//!   content_block_stop  (thinking)
//!   content_block_start (text)
//!   content_block_delta (text_delta)
//!   content_block_stop  (text)
//! message_delta
//! message_stop
//! ```
//!
//! The design implements Anthropic's Messages SSE contract as a small faithful
//! subset that covers the shapes our gateway actually emits: text plus
//! `reasoning_content` (DeepSeek R1 / o1 / o3 thinking). Tool-use blocks are
//! not yet emitted (the gateway currently has no tool-calling path).

use serde_json::json;

use crate::adapter::StreamUsage;

/// Stateful converter: turns OpenAI `chat.completion.chunk` `data:` frames into
/// Anthropic Messages SSE events. One instance lives for the whole upstream
/// stream and tracks which content block (thinking / text) is currently open so
/// the `content_block_start` / `content_block_stop` lifecycle is emitted exactly
/// once for each block, in the correct order (thinking before text).
pub struct AnthropicStreamState {
    pub model: String,
    pub message_id: String,
    started: bool,
    active_block: Option<BlockKind>,
    next_index: u32,
    finalized: bool,
    /// `input_tokens` reported in `message_start` (unknown at start → 0; the
    /// authoritative totals arrive in `message_delta` at stream end).
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Clone, Copy, PartialEq)]
enum BlockKind {
    Thinking,
    Text,
}

impl AnthropicStreamState {
    pub fn new(model: String, message_id: String) -> Self {
        Self {
            model,
            message_id,
            started: false,
            active_block: None,
            next_index: 0,
            finalized: false,
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    /// Emit `message_start` exactly once (lazily, on first use).
    pub fn ensure_started(&mut self, out: &mut String) {
        if self.started {
            return;
        }
        self.started = true;
        out.push_str(&message_start(self));
    }

    /// Consume one parsed Chat `chat.completion.chunk` JSON value, appending any
    /// Anthropic events it produces to `out`.
    pub fn on_chat_chunk(&mut self, json: &serde_json::Value, out: &mut String) {
        self.ensure_started(out);
        let Some(choices) = json.get("choices").and_then(|c| c.as_array()) else {
            return;
        };
        for choice in choices {
            let Some(delta) = choice.get("delta") else {
                continue;
            };
            // thinking (DeepSeek R1 / o1 / o3) -> Anthropic thinking block
            if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                if !reasoning.is_empty() {
                    self.emit_delta(
                        BlockKind::Thinking,
                        &json!({ "type": "thinking_delta", "thinking": reasoning }),
                        out,
                    );
                }
            }
            // text content -> Anthropic text block
            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    self.emit_delta(
                        BlockKind::Text,
                        &json!({ "type": "text_delta", "text": content }),
                        out,
                    );
                }
            }
        }
    }

    fn emit_delta(&mut self, kind: BlockKind, delta: &serde_json::Value, out: &mut String) {
        match self.active_block {
            None => {
                let idx = self.next_index;
                self.next_index += 1;
                self.active_block = Some(kind);
                out.push_str(&content_block_start(idx, kind));
            }
            Some(active) if active != kind => {
                // Close the current block, then open the new one.
                out.push_str(&content_block_stop(self.active_index()));
                let idx = self.next_index;
                self.next_index += 1;
                self.active_block = Some(kind);
                out.push_str(&content_block_start(idx, kind));
            }
            Some(_) => {}
        }
        out.push_str(&content_block_delta(self.active_index(), delta));
    }

    /// Index of the currently open block.
    fn active_index(&self) -> u32 {
        self.next_index.saturating_sub(1)
    }

    /// Emit the closing events: close any open block, then `message_delta`
    /// (carrying the scanned token usage) and `message_stop`. Idempotent.
    pub fn finalize(&mut self, usage: &StreamUsage, out: &mut String) {
        if self.finalized {
            return;
        }
        self.finalized = true;
        self.ensure_started(out);
        if self.active_block.is_some() {
            out.push_str(&content_block_stop(self.active_index()));
            self.active_block = None;
        }
        out.push_str(&message_delta(usage));
        out.push_str(&message_stop());
    }
}

// ---------------------------------------------------------------------------
// Event string builders
// ---------------------------------------------------------------------------

fn block_type_str(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Thinking => "thinking",
        BlockKind::Text => "text",
    }
}

fn message_start(st: &AnthropicStreamState) -> String {
    format!(
        "event: message_start\ndata: {}\n\n",
        json!({
            "type": "message_start",
            "message": {
                "id": st.message_id,
                "type": "message",
                "role": "assistant",
                "model": st.model,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": { "input_tokens": st.input_tokens, "output_tokens": 0 },
            }
        })
    )
}

fn content_block_start(index: u32, kind: BlockKind) -> String {
    let t = block_type_str(kind);
    let block = json!({ "type": t, "text": "" });
    format!(
        "event: content_block_start\ndata: {}\n\n",
        json!({
            "type": "content_block_start",
            "index": index,
            "content_block": block,
        })
    )
}

fn content_block_delta(index: u32, delta: &serde_json::Value) -> String {
    format!(
        "event: content_block_delta\ndata: {}\n\n",
        json!({
            "type": "content_block_delta",
            "index": index,
            "delta": delta,
        })
    )
}

fn content_block_stop(index: u32) -> String {
    format!(
        "event: content_block_stop\ndata: {}\n\n",
        json!({ "type": "content_block_stop", "index": index })
    )
}

fn message_delta(usage: &StreamUsage) -> String {
    format!(
        "event: message_delta\ndata: {}\n\n",
        json!({
            "type": "message_delta",
            "delta": { "stop_reason": "end_turn", "stop_sequence": null },
            "usage": {
                "input_tokens": usage.prompt_tokens,
                "output_tokens": usage.completion_tokens,
            },
        })
    )
}

fn message_stop() -> String {
    format!(
        "event: message_stop\ndata: {}\n\n",
        json!({ "type": "message_stop" })
    )
}
