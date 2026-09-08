//! Anthropic Messages <-> OpenAI Chat translation.
//!
//! Incoming `/v1/messages` requests are translated into the internal OpenAI
//! Chat shape (`anthropic_to_openai`); outgoing Chat completions are translated
//! back into the Anthropic Messages shape (`openai_to_anthropic`).
//!
//! Tool-calling is translated in both directions so an Anthropic-native client
//! (Claude Code 等) 走网关时 tool 链路不丢：
//!
//! ```text
//! tool_use      (assistant block) <-> message.tool_calls[]
//! tool_result   (user block)      <-> { role: "tool", tool_call_id }
//! tools[]       {name,description,input_schema} <-> tools[].function{name,description,parameters}
//! tool_choice   auto/any/tool/none              <-> auto/required/{function:{name}}/none
//! ```

use serde_json::{json, Value};

/// Convert an Anthropic Messages request into an OpenAI Chat request.
///
/// Handles `system` (string or `[{type:"text",text}]` blocks), `messages`
/// (string or blocks; `tool_use` -> `tool_calls`, `tool_result` -> a separate
/// `role:"tool"` message), `tools` / `tool_choice`, and forwards common sampler
/// params. `max_tokens` is `required` by Anthropic and maps directly onto the
/// same-named Chat field.
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
            match m.get("content") {
                Some(Value::Array(blocks)) => {
                    // 只取 text 块作为该条消息的文本；tool_result / tool_use 走各自
                    // 的结构化映射，避免内容被重复计入两处。
                    let text = {
                        let buf: String = blocks
                            .iter()
                            .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
                            .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
                            .collect();
                        if buf.is_empty() {
                            None
                        } else {
                            Some(Value::String(buf))
                        }
                    };
                    let tool_calls = tool_use_to_calls(blocks);
                    let results = tool_results(blocks);
                    let has_text = text.is_some();
                    if role == "assistant" && !tool_calls.is_empty() {
                        // assistant 带 tool_calls：content 可为 null（OpenAI 允许）
                        let mut msg = json!({ "role": "assistant", "tool_calls": tool_calls });
                        msg["content"] = text.unwrap_or(Value::Null);
                        messages.push(msg);
                        continue;
                    }
                    if let Some(t) = text {
                        messages.push(json!({ "role": role, "content": t }));
                    }
                    // user turn 里的 tool_result -> 独立的 role:"tool" 消息
                    for tr in results.iter() {
                        messages.push(tr.clone());
                    }
                    // 既无文本也无 tool 语义（图片等未识别块）：保持旧的透传行为，
                    // 避免静默丢弃
                    if !has_text
                        && tool_calls.is_empty()
                        && results.is_empty()
                        && !blocks.is_empty()
                    {
                        messages.push(json!({ "role": role, "content": blocks }));
                    }
                }
                Some(Value::String(s)) if !s.is_empty() => {
                    messages.push(json!({ "role": role, "content": s }));
                }
                _ => {}
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

    // tools: {name, description, input_schema} -> {type:function, function:{...}}
    if let Some(tools) = req.get("tools").and_then(|v| v.as_array()) {
        let mapped: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name").cloned().unwrap_or(json!("")),
                        "description": t.get("description").cloned().unwrap_or(json!("")),
                        "parameters": t.get("input_schema").cloned()
                            .unwrap_or(json!({ "type": "object", "properties": {} })),
                    }
                })
            })
            .collect();
        if !mapped.is_empty() {
            chat["tools"] = json!(mapped);
        }
    }

    // tool_choice: auto/any/tool/none -> auto/required/{function:{name}}/none
    if let Some(tc) = req.get("tool_choice") {
        if let Some(mapped) = tool_choice_to_openai(tc) {
            chat["tool_choice"] = mapped;
        }
    }

    chat
}

/// Anthropic `tool_choice` -> OpenAI `tool_choice`.
///
/// Anthropic 没有 `none` 之外的等价物差异，这里按语义最接近的映射；
/// 无法识别的形态返回 `None`（不下发字段，交给上游默认行为）。
fn tool_choice_to_openai(tc: &Value) -> Option<Value> {
    let kind = tc.get("type").and_then(|v| v.as_str()).unwrap_or("auto");
    match kind {
        "auto" => Some(json!("auto")),
        "any" => Some(json!("required")),
        "none" => Some(json!("none")),
        "tool" => {
            let name = tc.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                None
            } else {
                Some(json!({ "type": "function", "function": { "name": name } }))
            }
        }
        _ => None,
    }
}

/// Anthropic `tool_use` blocks -> OpenAI `tool_calls[]`.
fn tool_use_to_calls(blocks: &[Value]) -> Vec<Value> {
    let mut calls = Vec::new();
    for b in blocks {
        if b.get("type").and_then(|v| v.as_str()) != Some("tool_use") {
            continue;
        }
        let id = b
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let name = b
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let input = b.get("input").cloned().unwrap_or(json!({}));
        let arguments = serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
        calls.push(json!({
            "id": id,
            "type": "function",
            "function": { "name": name, "arguments": arguments },
        }));
    }
    calls
}

/// Anthropic `tool_result` blocks -> OpenAI `{role:"tool"}` messages.
fn tool_results(blocks: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for b in blocks {
        if b.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
            continue;
        }
        let tool_call_id = b
            .get("tool_use_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let content = match b.get("content") {
            Some(Value::String(s)) => Value::String(s.clone()),
            Some(Value::Array(arr)) => blocks_to_text(arr),
            Some(other) => other.clone(),
            None => Value::String(String::new()),
        };
        out.push(json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "content": content,
        }));
    }
    out
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
///
/// `tool_calls` 会转成 `tool_use` 内容块；纯 tool 轮不再插入空 text 块，
/// `stop_reason` 由 `finish_reason` 推导（`tool_calls` -> `tool_use`）。
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
    let choice = cc.get("choices").and_then(|c| c.get(0));
    let message = choice.and_then(|c| c.get("message"));
    let content_text = message
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let reasoning = message
        .and_then(|m| m.get("reasoning_content"))
        .and_then(|v| v.as_str());

    let tool_calls = message
        .and_then(|m| m.get("tool_calls"))
        .and_then(|v| v.as_array());
    let has_tool_calls = tool_calls.is_some_and(|a| !a.is_empty());

    let mut content_blocks: Vec<Value> = Vec::new();
    if let Some(r) = reasoning {
        if !r.is_empty() {
            content_blocks.push(json!({ "type": "thinking", "thinking": r }));
        }
    }
    // 纯 tool 轮不应夹带空 text 块；普通轮保留原有行为（至少一个 text 块）
    if !content_text.is_empty() || !has_tool_calls {
        content_blocks.push(json!({ "type": "text", "text": content_text }));
    }
    if let Some(calls) = tool_calls {
        for c in calls {
            let tid = c.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let fname = c
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args_raw = c
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let input: Value = serde_json::from_str(args_raw).unwrap_or_else(|_| json!({}));
            content_blocks.push(json!({
                "type": "tool_use",
                "id": tid,
                "name": fname,
                "input": input,
            }));
        }
    }

    let finish = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("stop");
    let stop_reason = match finish {
        "tool_calls" | "function_call" => "tool_use",
        "length" => "max_tokens",
        _ if has_tool_calls => "tool_use",
        _ => "end_turn",
    };

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
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": { "input_tokens": in_t, "output_tokens": out_t },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbound_tool_use_and_tool_result_map_to_openai() {
        let req = json!({
            "model": "claude-3",
            "max_tokens": 100,
            "tools": [{
                "name": "get_weather",
                "description": "查天气",
                "input_schema": { "type": "object", "properties": { "city": { "type": "string" } } }
            }],
            "messages": [
                { "role": "user", "content": "旧金山天气？" },
                { "role": "assistant", "content": [
                    { "type": "text", "text": "我来查一下" },
                    { "type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": { "city": "SF" } }
                ]},
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_1", "content": "18°C 晴" }
                ]}
            ]
        });
        let chat = anthropic_to_openai(&req);
        let msgs = chat["messages"].as_array().expect("messages 应为数组");
        assert_eq!(
            msgs.len(),
            3,
            "user / assistant(tool_calls) / tool: {msgs:?}"
        );
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], "我来查一下");
        assert_eq!(msgs[1]["tool_calls"][0]["function"]["name"], "get_weather");
        assert_eq!(
            msgs[1]["tool_calls"][0]["function"]["arguments"],
            r#"{"city":"SF"}"#
        );
        assert_eq!(msgs[2]["role"], "tool");
        assert_eq!(msgs[2]["tool_call_id"], "toolu_1");
        assert_eq!(msgs[2]["content"], "18°C 晴");

        // tools 定义同步转换
        assert_eq!(chat["tools"][0]["type"], "function");
        assert_eq!(chat["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(chat["tools"][0]["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn inbound_tool_choice_maps_to_openai() {
        let auto = json!({"model":"m","messages":[],"tool_choice":{"type":"auto"}});
        assert_eq!(anthropic_to_openai(&auto)["tool_choice"], json!("auto"));
        let any = json!({"model":"m","messages":[],"tool_choice":{"type":"any"}});
        assert_eq!(anthropic_to_openai(&any)["tool_choice"], json!("required"));
        let named =
            json!({"model":"m","messages":[],"tool_choice":{"type":"tool","name":"get_weather"}});
        assert_eq!(
            anthropic_to_openai(&named)["tool_choice"],
            json!({"type":"function","function":{"name":"get_weather"}})
        );
        // 未指定不下发字段
        let plain = json!({"model":"m","messages":[]});
        assert!(anthropic_to_openai(&plain).get("tool_choice").is_none());
    }

    #[test]
    fn outbound_tool_calls_become_tool_use_blocks() {
        let cc = json!({
            "id": "chatcmpl-1",
            "model": "claude-3",
            "created": 42,
            "choices": [{
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "get_weather", "arguments": "{\"city\":\"SF\"}" }
                    }]
                }
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
        });
        let a = openai_to_anthropic(&cc);
        assert_eq!(a["id"], "msg_1");
        assert_eq!(a["content"][0]["type"], "tool_use");
        assert_eq!(a["content"][0]["id"], "call_1");
        assert_eq!(a["content"][0]["name"], "get_weather");
        assert_eq!(a["content"][0]["input"], json!({"city":"SF"}));
        assert_eq!(a["stop_reason"], "tool_use");
        assert_eq!(a["usage"]["input_tokens"], 10);
        // 纯 tool 轮不应夹带空 text 块
        assert_eq!(a["content"].as_array().map(|v| v.len()), Some(1));
    }

    #[test]
    fn outbound_plain_text_keeps_end_turn() {
        let cc = json!({
            "id": "chatcmpl-2",
            "model": "m",
            "created": 1,
            "choices": [{ "finish_reason": "stop", "message": { "role": "assistant", "content": "hi" } }]
        });
        let a = openai_to_anthropic(&cc);
        assert_eq!(a["content"], json!([{"type":"text","text":"hi"}]));
        assert_eq!(a["stop_reason"], "end_turn");
    }

    #[test]
    fn outbound_length_maps_to_max_tokens() {
        let cc = json!({
            "id": "chatcmpl-3",
            "model": "m",
            "created": 1,
            "choices": [{ "finish_reason": "length", "message": { "role": "assistant", "content": "截" } }]
        });
        assert_eq!(openai_to_anthropic(&cc)["stop_reason"], "max_tokens");
    }

    #[test]
    fn inbound_plain_text_unchanged() {
        let req = json!({
            "model": "m",
            "system": "你是助手",
            "messages": [{ "role": "user", "content": "你好" }]
        });
        let chat = anthropic_to_openai(&req);
        assert_eq!(
            chat["messages"][0],
            json!({"role":"system","content":"你是助手"})
        );
        assert_eq!(chat["messages"][1], json!({"role":"user","content":"你好"}));
    }
}
