//! OpenAI Chat Completions SSE -> Anthropic Messages SSE.
//!
//! This is the outbound half of the `/v1/messages` bridge. The inbound half
//! (`anthropic_to_openai` in `crate::protocol::anthropic`) translates an Anthropic
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
//!   content_block_start (tool_use)   [only if tool_calls present]
//!   content_block_delta (input_json_delta)
//!   content_block_stop  (tool_use)
//! message_delta
//! message_stop
//! ```
//!
//! The design implements Anthropic's Messages SSE contract as a small faithful
//! subset that covers the shapes our gateway actually emits: text,
//! `reasoning_content` (DeepSeek R1 / o1 / o3 thinking) and tool-calling
//! (`tool_calls[]` -> `tool_use` blocks with `input_json_delta`).

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::adapter::StreamUsage;

/// Stateful converter: turns OpenAI `chat.completion.chunk` `data:` frames into
/// Anthropic Messages SSE events. One instance lives for the whole upstream
/// stream and tracks which content block (thinking / text / tool_use) is
/// currently open so the `content_block_start` / `content_block_stop` lifecycle
/// is emitted exactly once for each block, in the correct order (thinking
/// before text before tool_use).
pub struct AnthropicStreamState {
    pub model: String,
    pub message_id: String,
    started: bool,
    active_block: Option<BlockKey>,
    /// 当前打开块的 index（仅在 `active_block.is_some()` 时有意义）
    active_index: u32,
    next_index: u32,
    finalized: bool,
    /// `input_tokens` reported in `message_start` (unknown at start → 0; the
    /// authoritative totals arrive in `message_delta` at stream end).
    pub input_tokens: i64,
    #[allow(dead_code)]
    pub output_tokens: i64,
    /// OpenAI `tool_calls[].index` -> (id, name)。首帧之后只带 arguments 片段，
    /// 若该 tool 块被关闭后重现（少数上游会交错），靠这里补回身份信息。
    tool_meta: HashMap<u32, (String, String)>,
    /// 是否产出过 tool_use 块 —— 决定 `message_delta` 的 stop_reason。
    saw_tool_use: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum BlockKind {
    Thinking,
    Text,
}

#[derive(Clone, Copy, PartialEq)]
enum BlockKey {
    Thinking,
    Text,
    /// 携带 OpenAI 的 `tool_calls[].index`
    Tool(u32),
}

impl AnthropicStreamState {
    pub fn new(model: String, message_id: String) -> Self {
        Self {
            model,
            message_id,
            started: false,
            active_block: None,
            active_index: 0,
            next_index: 0,
            finalized: false,
            input_tokens: 0,
            output_tokens: 0,
            tool_meta: HashMap::new(),
            saw_tool_use: false,
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
    pub fn on_chat_chunk(&mut self, json: &Value, out: &mut String) {
        self.ensure_started(out);
        let Some(choices) = json.get("choices").and_then(|c| c.as_array()) else {
            return;
        };
        for choice in choices {
            // finish_reason=tool_calls 决定 message_delta 的 stop_reason
            if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                if fr == "tool_calls" || fr == "function_call" {
                    self.saw_tool_use = true;
                }
            }
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
            // tool_calls[] -> Anthropic tool_use blocks
            if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                for call in calls {
                    self.on_tool_call(call, out);
                }
            }
        }
    }

    /// 处理一个 `tool_calls[]` 增量片段：首帧带 id/name，后续帧只带 arguments。
    fn on_tool_call(&mut self, call: &Value, out: &mut String) {
        let idx = call.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let func = call.get("function");
        if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
            let name = func
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            self.tool_meta.insert(idx, (id.to_string(), name));
        } else if let Some(name) = func.and_then(|f| f.get("name")).and_then(|v| v.as_str()) {
            // 少数上游首帧不带 id，只带 name
            let entry = self
                .tool_meta
                .entry(idx)
                .or_insert_with(|| (String::new(), String::new()));
            if entry.1.is_empty() {
                entry.1 = name.to_string();
            }
        }

        let key = BlockKey::Tool(idx);
        if self.active_block != Some(key) {
            let (id, name) = self
                .tool_meta
                .get(&idx)
                .cloned()
                .unwrap_or_else(|| (String::new(), String::new()));
            self.open_block(
                key,
                json!({ "type": "tool_use", "id": id, "name": name, "input": {} }),
                out,
            );
        }
        self.saw_tool_use = true;

        if let Some(args) = func
            .and_then(|f| f.get("arguments"))
            .and_then(|v| v.as_str())
        {
            if !args.is_empty() {
                out.push_str(&content_block_delta(
                    self.active_index,
                    &json!({ "type": "input_json_delta", "partial_json": args }),
                ));
            }
        }
    }

    fn emit_delta(&mut self, kind: BlockKind, delta: &Value, out: &mut String) {
        let (key, block) = match kind {
            BlockKind::Thinking => (
                BlockKey::Thinking,
                json!({ "type": "thinking", "thinking": "" }),
            ),
            BlockKind::Text => (BlockKey::Text, json!({ "type": "text", "text": "" })),
        };
        self.open_block(key, block, out);
        out.push_str(&content_block_delta(self.active_index, delta));
    }

    /// 打开一个内容块；若当前已有别的块在开，先补 `content_block_stop`。
    fn open_block(&mut self, key: BlockKey, block: Value, out: &mut String) {
        if self.active_block == Some(key) {
            return;
        }
        if self.active_block.is_some() {
            out.push_str(&content_block_stop(self.active_index));
        }
        let idx = self.next_index;
        self.next_index += 1;
        self.active_block = Some(key);
        self.active_index = idx;
        out.push_str(&content_block_start(idx, &block));
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
            out.push_str(&content_block_stop(self.active_index));
            self.active_block = None;
        }
        let stop_reason = if self.saw_tool_use {
            "tool_use"
        } else {
            "end_turn"
        };
        out.push_str(&message_delta(usage, stop_reason));
        out.push_str(&message_stop());
    }
}

// ---------------------------------------------------------------------------
// Event string builders
// ---------------------------------------------------------------------------

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

fn content_block_start(index: u32, block: &Value) -> String {
    format!(
        "event: content_block_start\ndata: {}\n\n",
        json!({
            "type": "content_block_start",
            "index": index,
            "content_block": block,
        })
    )
}

fn content_block_delta(index: u32, delta: &Value) -> String {
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

fn message_delta(usage: &StreamUsage, stop_reason: &str) -> String {
    format!(
        "event: message_delta\ndata: {}\n\n",
        json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason, "stop_sequence": null },
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

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(p: i64, c: i64) -> StreamUsage {
        StreamUsage {
            prompt_tokens: p,
            completion_tokens: c,
            total_tokens: p + c,
            cached_tokens: 0,
        }
    }

    /// 包装成 chat.completion.chunk 的 choices[0].delta。
    fn chunk(delta: serde_json::Value) -> serde_json::Value {
        json!({ "choices": [ { "delta": delta } ] })
    }

    /// 抽取 SSE 文本里的事件名序列（按 `event: xxx` 行，空帧跳过）。
    fn event_names(sse: &str) -> Vec<&str> {
        sse.split("\n\n")
            .filter(|b| !b.trim().is_empty())
            .filter_map(|b| b.lines().find(|l| l.starts_with("event: ")))
            .map(|l| l.trim_start_matches("event: ").trim())
            .collect()
    }

    #[test]
    fn text_only_stream_emits_full_lifecycle() {
        let mut st = AnthropicStreamState::new("claude-x".into(), "msg_1".into());
        let mut out = String::new();
        st.on_chat_chunk(&chunk(json!({ "content": "Hel" })), &mut out);
        st.on_chat_chunk(&chunk(json!({ "content": "lo" })), &mut out);
        st.finalize(&usage(7, 3), &mut out);

        assert_eq!(
            event_names(&out),
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
        // 唯一的文本块 index=0
        assert!(
            out.contains("\"index\":0"),
            "文本块应从 index 0 开始: {out}"
        );
        assert!(out.contains("Hel") && out.contains("lo"));
        // message_delta 携带流末 usage
        assert!(out.contains("\"output_tokens\":3"));
        assert!(out.contains("\"stop_reason\":\"end_turn\""));
    }

    #[test]
    fn thinking_then_text_opens_two_blocks_in_order() {
        let mut st = AnthropicStreamState::new("claude-x".into(), "msg_2".into());
        let mut out = String::new();
        st.on_chat_chunk(&chunk(json!({ "reasoning_content": "step1" })), &mut out);
        st.on_chat_chunk(&chunk(json!({ "content": "answer" })), &mut out);
        st.finalize(&usage(1, 1), &mut out);

        assert_eq!(
            event_names(&out),
            vec![
                "message_start",
                "content_block_start", // thinking idx 0
                "content_block_delta",
                "content_block_stop",  // 关闭 thinking
                "content_block_start", // text idx 1
                "content_block_delta",
                "content_block_stop", // 关闭 text
                "message_delta",
                "message_stop",
            ]
        );
        // thinking 块必须排在 text 块之前
        let thinking_at = out.find("\"type\":\"thinking\"").expect("应有 thinking 块");
        let text_at = out.find("\"type\":\"text\"").expect("应有 text 块");
        assert!(thinking_at < text_at, "thinking 必须排在 text 之前");
    }

    #[test]
    fn finalize_is_idempotent() {
        let mut st = AnthropicStreamState::new("m".into(), "msg_3".into());
        let mut out = String::new();
        st.on_chat_chunk(&chunk(json!({ "content": "x" })), &mut out);
        st.finalize(&usage(1, 1), &mut out);
        st.finalize(&usage(1, 1), &mut out); // 重复调用应无副作用

        assert_eq!(out.matches("event: message_stop").count(), 1);
        assert_eq!(out.matches("event: message_delta").count(), 1);
    }

    #[test]
    fn empty_delta_does_not_open_block() {
        let mut st = AnthropicStreamState::new("m".into(), "msg_4".into());
        let mut out = String::new();
        st.on_chat_chunk(&chunk(json!({ "content": "" })), &mut out);
        st.on_chat_chunk(&chunk(json!({ "reasoning_content": "" })), &mut out);

        // 只应留下惰性发出的 message_start，不产生任何 content block
        assert_eq!(event_names(&out), vec!["message_start"]);
    }

    #[test]
    fn chunk_without_choices_is_ignored() {
        let mut st = AnthropicStreamState::new("m".into(), "msg_5".into());
        let mut out = String::new();
        st.on_chat_chunk(&json!({ "object": "chat.completion.chunk" }), &mut out);

        assert_eq!(event_names(&out), vec!["message_start"]);
    }

    #[test]
    fn tool_call_stream_emits_tool_use_block() {
        let mut st = AnthropicStreamState::new("claude-x".into(), "msg_6".into());
        let mut out = String::new();
        // 首帧带身份，后续帧只带 arguments 片段
        st.on_chat_chunk(
            &chunk(
                json!({ "tool_calls": [{ "index": 0, "id": "toolu_1", "type": "function",
                "function": { "name": "get_weather", "arguments": "{\"c" } }] }),
            ),
            &mut out,
        );
        st.on_chat_chunk(
            &chunk(json!({ "tool_calls": [{ "index": 0,
                "function": { "arguments": "ity\":\"SF\"}" } }] })),
            &mut out,
        );
        st.on_chat_chunk(
            &json!({ "choices": [ { "delta": {}, "finish_reason": "tool_calls" } ] }),
            &mut out,
        );
        st.finalize(&usage(9, 4), &mut out);

        assert_eq!(
            event_names(&out),
            vec![
                "message_start",
                "content_block_start", // tool_use idx 0
                "content_block_delta", // 第 1 段 arguments
                "content_block_delta", // 第 2 段 arguments
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
        // tool_use 块的身份信息
        assert!(
            out.contains("\"type\":\"tool_use\""),
            "应有 tool_use 块: {out}"
        );
        assert!(out.contains("\"id\":\"toolu_1\""));
        assert!(out.contains("\"name\":\"get_weather\""));
        // arguments 分片通过 input_json_delta 转发
        assert_eq!(out.matches("input_json_delta").count(), 2);
        assert!(out.contains("partial_json"));
        // finish_reason=tool_calls -> stop_reason=tool_use
        assert!(out.contains("\"stop_reason\":\"tool_use\""), "{out}");
    }

    #[test]
    fn text_then_tool_opens_two_blocks() {
        let mut st = AnthropicStreamState::new("claude-x".into(), "msg_7".into());
        let mut out = String::new();
        st.on_chat_chunk(&chunk(json!({ "content": "我先查一下" })), &mut out);
        st.on_chat_chunk(
            &chunk(
                json!({ "tool_calls": [{ "index": 0, "id": "toolu_9", "type": "function",
                "function": { "name": "search", "arguments": "{}" } }] }),
            ),
            &mut out,
        );
        st.finalize(&usage(1, 1), &mut out);

        assert_eq!(
            event_names(&out),
            vec![
                "message_start",
                "content_block_start", // text idx 0
                "content_block_delta",
                "content_block_stop",  // 关闭 text
                "content_block_start", // tool_use idx 1
                "content_block_delta",
                "content_block_stop", // 关闭 tool_use
                "message_delta",
                "message_stop",
            ]
        );
        // tool_use 必须是 index 1，不能复用 text 的 index
        assert!(out.contains("\"index\":1"), "{out}");
        assert!(out.contains("\"stop_reason\":\"tool_use\""));
    }
}
