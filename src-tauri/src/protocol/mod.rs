//! Protocol translation layer.
//!
//! DongX's internal representation is always OpenAI Chat. The three downstream
//! protocols — OpenAI Chat Completions, Anthropic Messages, OpenAI Responses —
//! are translated to/from that internal shape here, mirroring the reference
//! implementation's `protocol/` module (the four core conversions correspond to
//! waliapi's `protocol::legacy::{anthropic_to_openai, openai_to_anthropic,
//! responses_to_openai, openai_to_responses}`).
//!
//! Both the non-streaming request/response translation and the streaming
//! reverse-conversion live in this module (mirroring waliapi, where the
//! protocol owns conversion for both sync and streaming):
//! - `anthropic` / `anthropic_stream`: Anthropic Messages <-> OpenAI Chat
//!   (sync + streaming SSE).
//! - `responses` / `responses_stream`: OpenAI Responses <-> OpenAI Chat
//!   (sync + streaming SSE).
//! - Upstream dialect SSE (Claude/Gemini native -> Chat) stays in
//!   `crate::adapter` (`AnthropicSseConverter` / `GeminiSseConverter`), which is
//!   the adapter concern, not the protocol concern.
pub mod anthropic;
pub mod responses;
pub(crate) mod anthropic_stream;
pub(crate) mod responses_stream;

pub(crate) use anthropic::{anthropic_to_openai, openai_to_anthropic};
pub(crate) use responses::{openai_to_responses, responses_to_openai};
