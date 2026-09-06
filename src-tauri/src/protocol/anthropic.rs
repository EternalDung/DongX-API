//! Anthropic Messages <-> OpenAI Chat translation.
//!
//! Incoming `/v1/messages` requests are translated into the internal OpenAI
//! Chat shape (`anthropic_to_openai`); outgoing Chat completions are translated
//! back into the Anthropic Messages shape (`openai_to_anthropic`).

use serde_json::{json, Value};

/// Convert an Anthropic Messages request into an OpenAI Chat request.
///
/// Handles `system` (string or `[{type:"text",text}]` blocks), `messages`
/// (string or `[{type:"text",text}]` blocks; tool_result text is extracted
/// best-effort), and forwards common sampler params (`temperature`, `top_p`,
/// `max_tokens`, `stop`, `top_k`, `seed`). `max_tokens` is `required` by
/// Anthropic and maps directly onto the same-named Chat field.
pub(crate) fn anthropic_to_openai(req: &Value) -> Value {
    let mut messages: Vec<Value> = Vec::new();

    // system -> Chat system message
    if let Some(sys) = req.get("system") {
        let text = match sys {
            Value::String(s) => Value::String(s.clone()),
            Value::Array(arr) => blocks_to_text(arr),
            _ => Value::Null,
        };
        if let Value::String(t) = &text {
            if !t.is_empty() {
                messages.push(json!({ "role": "system", "content": t }));
            }
        }
    }

    if let Some(msgs) = req.get("messages").and_then(|v| v.as_array()) {
        for m in msgs {
            let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = m.get("content");
            let text = match content {
                Some(Value::String(s)) => Value::String(s.clone()),
                Some(Value::Array(arr)) => {
                    let t = blocks_to_text(arr);
                    if let Value::String(s) = &t {
                        if s.is_empty() {
                            Value::Null
                        } else {
                            t
                        }
                    } else {
                        Value::Null
                    }
                }
                _ => Value::Null,
            };
            if text.is_null() {
                // Fall back to passing the raw content through (e.g. tool_use
                // blocks) — best effort; the upstream adapts what it can.
                messages.push(json!({ "role": role, "content": content }));
            } else {
                messages.push(json!({ "role": role, "content": text }));
            }
        }
    }

    let mut chat = json!({
        "model": req.get("model").cloned().unwrap_or(json!("")),
        "messages": messages,
        "stream": req.get("stream").cloned().unwrap_or(json!(false)),
    });
    for f in [
        "temperature",
        "top_p",
        "top_k",
        "max_tokens",
        "stop",
        "seed",
    ] {
        if let Some(v) = req.get(f) {
            chat[f] = v.clone();
        }
    }
    chat
}

/// Concatenate the text of `[{type:"text",text}, ...]` content blocks.
fn blocks_to_text(blocks: &[Value]) -> Value {
    let mut buf = String::new();
    for b in blocks {
        let t = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match t {
            "text" => {
                if let Some(s) = b.get("text").and_then(|v| v.as_str()) {
                    buf.push_str(s);
                }
            }
            "tool_result" => {
                if let Some(c) = b.get("content").and_then(|v| v.as_str()) {
                    buf.push_str(c);
                }
            }
            _ => {}
        }
    }
    Value::String(buf)
}

/// Convert an OpenAI Chat completion into the Anthropic Messages shape.
pub(crate) fn openai_to_anthropic(cc: &Value) -> Value {
    let id = cc
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("msg_unknown");
    // OpenAI ids look like "chatcmpl-xxx"; Anthropic message ids are "msg_...".
    let anthropic_id = if let Some(core) = id.strip_prefix("chatcmpl-") {
        format!("msg_{}", core)
    } else {
        id.to_string()
    };
    let model = cc.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let created = cc.get("created").and_then(|v| v.as_i64()).unwrap_or(0);
    let message = cc
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"));
    let content_text = message
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let reasoning = message
        .and_then(|m| m.get("reasoning_content"))
        .and_then(|v| v.as_str());

    let mut content_blocks: Vec<Value> = Vec::new();
    if let Some(r) = reasoning {
        if !r.is_empty() {
            content_blocks.push(json!({ "type": "thinking", "thinking": r }));
        }
    }
    content_blocks.push(json!({ "type": "text", "text": content_text }));

    let usage = cc.get("usage");
    let (in_t, out_t) = match usage {
        Some(u) => (
            u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            u.get("completion_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        ),
        None => (0, 0),
    };

    json!({
        "id": anthropic_id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "created_at": created,
        "content": content_blocks,
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": { "input_tokens": in_t, "output_tokens": out_t },
    })
}
