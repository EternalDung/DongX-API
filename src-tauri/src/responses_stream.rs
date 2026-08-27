//! Streaming converter: OpenAI Chat Completions SSE -> OpenAI Responses SSE.
//!
//! This is the outbound half of the Responses bridge. The inbound half
//! (`responses_input_to_messages` in `server/handler.rs`) translates a
//! Responses `input` into Chat `messages`; the shared chat pipeline then runs
//! exactly like `/v1/chat/completions` and yields a Chat SSE stream. This
//! module turns those Chat `chat.completion.chunk` frames into the Responses
//! event stream (`response.created` -> `response.output_item.added` ->
//! `response.output_text.delta` -> `response.completed` + `data: [DONE]`).
//!
//! The design implements OpenAI's Responses SSE event contract
//! (response.created -> output_item.added -> output_text.delta ->
//! response.completed), kept as a small faithful subset that covers the three item kinds our gateway
//! actually emits: text, reasoning (`reasoning_content`), and tool_calls.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;

use crate::adapter::StreamUsage;

/// Monotonic counter used to mint unique `resp_*` ids without extra deps.
static RESP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Mint a response id (`resp_<millis><seq>`). Good enough for a local gateway
/// and keeps the `resp_*` shape OpenAI clients expect.
pub fn new_response_id() -> String {
    let n = RESP_SEQ.fetch_add(1, Ordering::SeqCst);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("resp_{}{}", now, n)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn msg_id_of(response_id: &str) -> String {
    if let Some(core) = response_id.strip_prefix("resp_") {
        format!("msg_{}", core)
    } else {
        format!("msg_{}", response_id)
    }
}

fn reasoning_id_of(response_id: &str) -> String {
    if let Some(core) = response_id.strip_prefix("resp_") {
        format!("rs_{}", core)
    } else {
        format!("rs_{}", response_id)
    }
}

/// Per-output-item streaming state for the Chat-SSE -> Responses-SSE converter.
#[derive(Default)]
pub struct ResponsesStreamState {
    /// Text message `output_item.added` already emitted.
    pub text_item_added: bool,
    /// Text message `output_item.done` already emitted.
    pub text_item_done: bool,
    /// Text `content_part.added` already emitted.
    pub text_part_added: bool,
    /// `output_index` assigned to the text message item.
    pub text_output_index: u32,
    /// Next `output_index` to assign to a new item.
    pub next_output_index: u32,
    /// Reasoning `output_item.added` already emitted.
    pub reasoning_item_added: bool,
    /// Reasoning `output_item.done` already emitted.
    pub reasoning_item_done: bool,
    /// Reasoning `reasoning_summary_part.added` already emitted.
    pub reasoning_part_added: bool,
    /// `output_index` assigned to the reasoning item.
    pub reasoning_output_index: u32,
    /// Full concatenated reasoning text.
    pub accumulated_reasoning: String,
    /// Full concatenated text content (the final `output_text`).
    pub accumulated_content: String,
    /// Map of tool_call index -> per-tool-call state.
    pub tool_calls: HashMap<u64, ToolCallState>,
    /// Whether any tool calls were seen.
    pub has_tool_calls: bool,
    /// Whether `response.completed` has been emitted.
    pub completed_sent: bool,
    /// Monotonic sequence number for all events.
    pub sequence_number: u64,
}

/// Per-tool-call streaming state.
#[derive(Clone)]
pub struct ToolCallState {
    pub output_index: u32,
    pub call_id: String,
    pub name: String,
    pub item_id: String,
    pub accumulated_arguments: String,
    pub item_added_sent: bool,
    pub arguments_done_sent: bool,
    pub output_item_done_sent: bool,
}

/// Opening events: `response.created` + `response.in_progress`.
pub fn created_events(response_id: &str, model: &str) -> String {
    let response_obj = json!({
        "id": response_id,
        "object": "response",
        "created_at": now_secs(),
        "status": "in_progress",
        "model": model,
        "output": [],
        "parallel_tool_calls": false,
        "tool_choice": "auto",
        "tools": [],
        "top_p": null,
        "temperature": null,
        "truncation": null,
        "usage": null,
        "background": false,
        "completed_at": null,
    });
    let created = json!({
        "type": "response.created",
        "response": response_obj,
        "sequence_number": 0,
    });
    let in_progress = json!({
        "type": "response.in_progress",
        "response": response_obj,
        "sequence_number": 1,
    });
    format!(
        "event: response.created\ndata: {}\n\nevent: response.in_progress\ndata: {}\n\n",
        created, in_progress
    )
}

/// Convert one chunk of Chat SSE text into zero or more Responses SSE events.
///
/// `chunk` may contain multiple `data:` frames. Each frame is parsed as a
/// Chat `chat.completion.chunk`; `choices[].delta` drives the Responses event
/// chain. The `state` is mutated so the item lifecycle (`*_added` / `*_done`)
/// is emitted exactly once across the whole stream.
pub fn convert_chunk(
    chunk: &str,
    response_id: &str,
    state: &mut ResponsesStreamState,
) -> Vec<String> {
    let mut events = Vec::new();
    let msg_id = msg_id_of(response_id);
    let reasoning_id = reasoning_id_of(response_id);

    for line in chunk.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("data:") {
            continue;
        }
        let data_str = trimmed.trim_start_matches("data:").trim();
        if data_str.is_empty() || data_str == "[DONE]" {
            continue;
        }
        let json: serde_json::Value = match serde_json::from_str(data_str) {
            Ok(j) => j,
            Err(_) => continue,
        };
        let Some(choices) = json.get("choices").and_then(|c| c.as_array()) else {
            continue;
        };
        for choice in choices {
            let Some(delta) = choice.get("delta") else {
                continue;
            };

            // --- reasoning_content (DeepSeek R1 / o1 / o3) ---
            if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                if !reasoning.is_empty() {
                    if !state.reasoning_item_added {
                        let idx = state.next_output_index;
                        state.reasoning_output_index = idx;
                        state.sequence_number += 1;
                        let item = json!({
                            "id": reasoning_id,
                            "type": "reasoning",
                            "status": "in_progress",
                            "summary": [],
                            "content": [],
                        });
                        events.push(format!(
                            "event: response.output_item.added\ndata: {}\n\n",
                            json!({
                                "type": "response.output_item.added",
                                "output_index": idx,
                                "item": item,
                                "sequence_number": state.sequence_number,
                            })
                        ));
                        state.sequence_number += 1;
                        let part = json!({ "type": "reasoning_summary_text", "text": "" });
                        events.push(format!(
                            "event: response.reasoning_summary_part.added\ndata: {}\n\n",
                            json!({
                                "type": "response.reasoning_summary_part.added",
                                "item_id": reasoning_id,
                                "output_index": idx,
                                "summary_index": 0,
                                "part": part,
                                "sequence_number": state.sequence_number,
                            })
                        ));
                        state.reasoning_item_added = true;
                        state.reasoning_part_added = true;
                        state.next_output_index += 1;
                    }
                    state.accumulated_reasoning.push_str(reasoning);
                    state.sequence_number += 1;
                    events.push(format!(
                        "event: response.reasoning_summary_text.delta\ndata: {}\n\n",
                        json!({
                            "type": "response.reasoning_summary_text.delta",
                            "item_id": reasoning_id,
                            "output_index": state.reasoning_output_index,
                            "summary_index": 0,
                            "delta": reasoning,
                            "sequence_number": state.sequence_number,
                        })
                    ));
                }
            }

            // --- text content delta ---
            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                if !content.is_empty() {
                    if !state.text_item_added {
                        let idx = state.next_output_index;
                        state.text_output_index = idx;
                        state.sequence_number += 1;
                        let item = json!({
                            "id": msg_id,
                            "type": "message",
                            "status": "in_progress",
                            "role": "assistant",
                            "content": [],
                        });
                        events.push(format!(
                            "event: response.output_item.added\ndata: {}\n\n",
                            json!({
                                "type": "response.output_item.added",
                                "output_index": idx,
                                "item": item,
                                "sequence_number": state.sequence_number,
                            })
                        ));
                        state.sequence_number += 1;
                        let part = json!({ "type": "output_text", "text": "", "annotations": [] });
                        events.push(format!(
                            "event: response.content_part.added\ndata: {}\n\n",
                            json!({
                                "type": "response.content_part.added",
                                "item_id": msg_id,
                                "output_index": idx,
                                "content_index": 0,
                                "part": part,
                                "sequence_number": state.sequence_number,
                            })
                        ));
                        state.text_item_added = true;
                        state.text_part_added = true;
                        state.next_output_index += 1;
                    }
                    state.accumulated_content.push_str(content);
                    state.sequence_number += 1;
                    events.push(format!(
                        "event: response.output_text.delta\ndata: {}\n\n",
                        json!({
                            "type": "response.output_text.delta",
                            "item_id": msg_id,
                            "output_index": state.text_output_index,
                            "content_index": 0,
                            "delta": content,
                            "sequence_number": state.sequence_number,
                        })
                    ));
                }
            }

            // --- function_call (tool_calls) delta ---
            if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                state.has_tool_calls = true;
                for tc in tool_calls {
                    let tc_index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                    let tc_id = tc
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|a| a.as_str())
                        .unwrap_or("")
                        .to_string();

                    let tc_state = state.tool_calls.entry(tc_index).or_insert_with(|| {
                        let output_index = state.next_output_index;
                        state.next_output_index += 1;
                        let item_id = if !tc_id.is_empty() {
                            tc_id.clone()
                        } else {
                            format!("fc_{}", tc_index)
                        };
                        ToolCallState {
                            output_index,
                            call_id: tc_id.clone(),
                            name: name.clone(),
                            item_id: item_id.clone(),
                            accumulated_arguments: String::new(),
                            item_added_sent: false,
                            arguments_done_sent: false,
                            output_item_done_sent: false,
                        }
                    });
                    if tc_state.call_id.is_empty() && !tc_id.is_empty() {
                        tc_state.call_id = tc_id;
                    }
                    if tc_state.name.is_empty() && !name.is_empty() {
                        tc_state.name = name;
                    }
                    if !tc_state.item_added_sent {
                        state.sequence_number += 1;
                        let fc_item = json!({
                            "id": tc_state.item_id,
                            "type": "function_call",
                            "status": "in_progress",
                            "call_id": tc_state.call_id,
                            "name": tc_state.name,
                            "arguments": "",
                        });
                        events.push(format!(
                            "event: response.output_item.added\ndata: {}\n\n",
                            json!({
                                "type": "response.output_item.added",
                                "output_index": tc_state.output_index,
                                "item": fc_item,
                                "sequence_number": state.sequence_number,
                            })
                        ));
                        tc_state.item_added_sent = true;
                    }
                    if !arguments.is_empty() && !tc_state.arguments_done_sent {
                        tc_state.accumulated_arguments.push_str(&arguments);
                        state.sequence_number += 1;
                        events.push(format!(
                            "event: response.function_call_arguments.delta\ndata: {}\n\n",
                            json!({
                                "type": "response.function_call_arguments.delta",
                                "item_id": tc_state.item_id,
                                "output_index": tc_state.output_index,
                                "delta": arguments,
                                "sequence_number": state.sequence_number,
                            })
                        ));
                    }
                }
            }
        }
    }
    events
}

/// Closing events: finish open items, then `response.completed` (+ `[DONE]`).
///
/// Called once when the upstream Chat SSE ends. `usage` carries the tokens
/// scanned from the stream (it only becomes known on the final frame).
pub fn completed_events(
    response_id: &str,
    model: &str,
    state: &mut ResponsesStreamState,
    usage: &StreamUsage,
) -> Vec<String> {
    let mut events = Vec::new();
    let msg_id = msg_id_of(response_id);
    let reasoning_id = reasoning_id_of(response_id);
    let mut seq = state.sequence_number;

    macro_rules! next_seq {
        () => {{
            seq += 1;
            seq
        }};
    }

    // Close reasoning item if it was opened.
    if state.reasoning_item_added && !state.reasoning_item_done {
        next_seq!();
        events.push(format!(
            "event: response.reasoning_summary_text.done\ndata: {}\n\n",
            json!({
                "type": "response.reasoning_summary_text.done",
                "item_id": reasoning_id,
                "output_index": state.reasoning_output_index,
                "summary_index": 0,
                "text": state.accumulated_reasoning,
                "sequence_number": seq,
            })
        ));
        next_seq!();
        let part = json!({ "type": "reasoning_summary_text", "text": state.accumulated_reasoning });
        events.push(format!(
            "event: response.reasoning_summary_part.done\ndata: {}\n\n",
            json!({
                "type": "response.reasoning_summary_part.done",
                "item_id": reasoning_id,
                "output_index": state.reasoning_output_index,
                "summary_index": 0,
                "part": part,
                "sequence_number": seq,
            })
        ));
        next_seq!();
        let completed_item = json!({
            "id": reasoning_id,
            "type": "reasoning",
            "status": "completed",
            "summary": [{ "type": "summary_text", "text": state.accumulated_reasoning }],
            "content": [],
        });
        events.push(format!(
            "event: response.output_item.done\ndata: {}\n\n",
            json!({
                "type": "response.output_item.done",
                "output_index": state.reasoning_output_index,
                "item": completed_item,
                "sequence_number": seq,
            })
        ));
        state.reasoning_item_done = true;
    }

    // Close text item if it was opened.
    if state.text_item_added && !state.text_item_done {
        next_seq!();
        events.push(format!(
            "event: response.output_text.done\ndata: {}\n\n",
            json!({
                "type": "response.output_text.done",
                "item_id": msg_id,
                "output_index": state.text_output_index,
                "content_index": 0,
                "text": state.accumulated_content,
                "sequence_number": seq,
            })
        ));
        next_seq!();
        let part = json!({
            "type": "output_text",
            "text": state.accumulated_content,
            "annotations": [],
        });
        events.push(format!(
            "event: response.content_part.done\ndata: {}\n\n",
            json!({
                "type": "response.content_part.done",
                "item_id": msg_id,
                "output_index": state.text_output_index,
                "content_index": 0,
                "part": part,
                "sequence_number": seq,
            })
        ));
        next_seq!();
        let completed_item = json!({
            "id": msg_id,
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": state.accumulated_content, "annotations": [] }],
        });
        events.push(format!(
            "event: response.output_item.done\ndata: {}\n\n",
            json!({
                "type": "response.output_item.done",
                "output_index": state.text_output_index,
                "item": completed_item,
                "sequence_number": seq,
            })
        ));
        state.text_item_done = true;
    }

    // Close any still-open tool call items.
    for tc_state in state.tool_calls.values() {
        let effective_call_id = if tc_state.call_id.is_empty() {
            format!("call_{}", tc_state.output_index)
        } else {
            tc_state.call_id.clone()
        };
        if !tc_state.arguments_done_sent {
            next_seq!();
            events.push(format!(
                "event: response.function_call_arguments.done\ndata: {}\n\n",
                json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": tc_state.item_id,
                    "output_index": tc_state.output_index,
                    "name": tc_state.name,
                    "arguments": tc_state.accumulated_arguments,
                    "sequence_number": seq,
                })
            ));
        }
        if !tc_state.output_item_done_sent {
            next_seq!();
            let fc_completed = json!({
                "id": tc_state.item_id,
                "type": "function_call",
                "status": "completed",
                "call_id": effective_call_id,
                "name": tc_state.name,
                "arguments": tc_state.accumulated_arguments,
            });
            events.push(format!(
                "event: response.output_item.done\ndata: {}\n\n",
                json!({
                    "type": "response.output_item.done",
                    "output_index": tc_state.output_index,
                    "item": fc_completed,
                    "sequence_number": seq,
                })
            ));
        }
    }

    // Build the final `output` array for response.completed.
    let mut output_items: Vec<serde_json::Value> = Vec::new();
    if state.reasoning_item_added {
        output_items.push(json!({
            "id": reasoning_id,
            "type": "reasoning",
            "status": "completed",
            "summary": [{ "type": "summary_text", "text": state.accumulated_reasoning }],
            "content": [],
        }));
    }
    if state.text_item_added {
        output_items.push(json!({
            "id": msg_id,
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": state.accumulated_content, "annotations": [] }],
        }));
    }
    for tc_state in state.tool_calls.values() {
        let effective_call_id = if tc_state.call_id.is_empty() {
            format!("call_{}", tc_state.output_index)
        } else {
            tc_state.call_id.clone()
        };
        output_items.push(json!({
            "id": tc_state.item_id,
            "type": "function_call",
            "status": "completed",
            "call_id": effective_call_id,
            "name": tc_state.name,
            "arguments": tc_state.accumulated_arguments,
        }));
    }

    let response_obj = json!({
        "id": response_id,
        "object": "response",
        "created_at": now_secs(),
        "status": "completed",
        "model": model,
        "output": output_items,
        "parallel_tool_calls": false,
        "tool_choice": "auto",
        "tools": [],
        "top_p": null,
        "temperature": null,
        "truncation": null,
        "usage": {
            "input_tokens": usage.prompt_tokens,
            "output_tokens": usage.completion_tokens,
            "total_tokens": usage.total_tokens,
        },
        "background": false,
        "completed_at": now_secs(),
    });
    next_seq!();
    events.push(format!(
        "event: response.completed\ndata: {}\n\n",
        json!({
            "type": "response.completed",
            "response": response_obj,
            "sequence_number": seq,
        })
    ));
    events.push("data: [DONE]\n\n".to_string());

    state.completed_sent = true;
    state.sequence_number = seq;
    events
}
