//! OpenAI Responses <-> OpenAI Chat translation.
//!
//! Incoming `/v1/responses` requests have their `input` (plus optional
//! top-level `instructions`) translated into the internal OpenAI Chat
//! `messages` (`responses_to_openai`); outgoing Chat completions are translated
//! back into the Responses API shape (`openai_to_responses`).

use serde_json::{json, Value};

/// Convert a Responses `input` (plus optional top-level `instructions`) into
/// OpenAI Chat `messages`.
pub(crate) fn responses_to_openai(req: &Value) -> Vec<Value> {
    let mut messages = Vec::new();
    if let Some(instr) = req.get("instructions").and_then(|v| v.as_str()) {
        if !instr.is_empty() {
            messages.push(json!({ "role": "system", "content": instr }));
        }
    }
    if let Some(input) = req.get("input") {
        match input {
            Value::String(s) => {
                messages.push(json!({ "role": "user", "content": s }));
            }
            Value::Array(items) => {
                for item in items {
                    let role = item
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("user");
                    let text = match item.get("content") {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Array(parts)) => {
                            let mut buf = String::new();
                            for p in parts {
                                if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                                    buf.push_str(t);
                                }
                            }
                            buf
                        }
                        _ => String::new(),
                    };
                    messages.push(json!({ "role": role, "content": text }));
                }
            }
            _ => {}
        }
    }
    messages
}

/// Convert an OpenAI Chat completion into the Responses API shape.
pub(crate) fn openai_to_responses(cc: &Value) -> Value {
    let model = cc.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let created = cc.get("created").and_then(|v| v.as_i64()).unwrap_or(0);
    let id = cc.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
    let id_core = id.trim_start_matches("chatcmpl-");
    let choice = cc.get("choices").and_then(|c| c.get(0));
    let message = choice.and_then(|c| c.get("message"));
    let content_text = message
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let reasoning = message
        .and_then(|m| m.get("reasoning_content"))
        .and_then(|v| v.as_str());
    let mut content_items: Vec<Value> = Vec::new();
    if let Some(r) = reasoning {
        if !r.is_empty() {
            content_items.push(json!({ "type": "reasoning", "summary": [r] }));
        }
    }
    content_items.push(json!({ "type": "output_text", "text": content_text }));
    let usage = cc.get("usage");
    let (in_t, out_t, tot_t) = match usage {
        Some(u) => (
            u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            u.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            u.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
        ),
        None => (0, 0, 0),
    };
    json!({
        "id": format!("resp_{}", id_core),
        "object": "response",
        "created_at": created,
        "model": model,
        "status": "completed",
        "output": [
            {
                "type": "message",
                "id": format!("msg_{}", id_core),
                "role": "assistant",
                "status": "completed",
                "content": content_items,
            }
        ],
        "usage": {
            "input_tokens": in_t,
            "output_tokens": out_t,
            "total_tokens": tot_t,
        }
    })
}
