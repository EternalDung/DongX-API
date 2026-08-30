# DongX

> Local LLM API Gateway · Personal Project (Side Project)

DongX is a local-first LLM API gateway desktop app. It consolidates multiple providers' APIs into a single OpenAI-compatible endpoint and handles key management, weighted routing, failover, and security auditing entirely on your machine — no external services, and all your data stays local.

> 中文文档见 [README.md](./README.md)。

## Features

### Unified Access & Protocol Adaptation
- Supports **OpenAI / Anthropic / Gemini (native API)** protocol adapters, exposing OpenAI-compatible `/v1/chat/completions` and `/v1/responses` (Responses API mode).
- **Both streaming and non-streaming pipelines** are wired end-to-end: OpenAI-family responses pass through; Claude / Gemini native SSE is converted chunk-by-chunk into OpenAI SSE by the adapters; the Responses mode is built by transcribing Chat SSE frame-by-frame.
- The channel selector is protocol-driven (OpenAI / Anthropic / Ollama); the provider dropdown is filtered by protocol, and a single vendor can be mounted across protocols (e.g. DeepSeek on both OpenAI- and Anthropic-compatible endpoints).

### Weighted Routing & Load Balancing
- Multiple models under the same provider are distributed by **weight**.
- Each key has an **independent token budget**; `usage.total_tokens` is accumulated per response and a key is auto-disabled and removed from routing candidates once it exceeds the budget.
- Multi-key **weighted load balancing**: `keys: Vec<{key,weight}>` is stored encrypted and dispatched by weight.

### High Availability: Circuit Breaking & Failover
- **Channel-level circuit breaker**: after a threshold of consecutive failures (default 3) a channel is cooled down (default 60s) and excluded from dispatch while cooling.
- **In-request failover (state machine)**: automatically retries on the next candidate channel within a single request; retryable failures (5xx / 429 / 408 / 409 / connection timeout) switch channels, while 4xx is not retried; the retry cap is `retry_times + 1`.
- Upstream jitter is transparent to the caller.

### Security Auditing
- **25 built-in detection rules** covering credentials, PII, payment cards, command injection, Unicode steganography, network egress, tool risk, and prompt injection — grouped by category, each individually toggleable and severity-adjustable.
- **Custom blacklist rules**: a hit can warn (warn) or block (block).
- **Three-phase auditing**: request body / response body / streaming delta (response_delta) are all scanned; the streaming phase only records and never blocks.
- **Evidence masking + forensic hash**: matched evidence is masked before storage, and a SHA-256 `evidence_hash` is computed for cross-phase deduplication and tracing — **plaintext is never persisted**.
- **Gateway-level rate limiting**: per-gateway-key RPM limiting; exceeding the limit returns `429`.
- Four security gate modes: `audit` (log only) / `warn` (alert) / `redact` (mask-and-forward) / `block` (block locally).
- Built-in rule management UI in Settings: grouping / search / severity adjustment / toggle / reset-to-defaults / one-click collapse / gated by global toggles / enabled-count badge.

### Key & Configuration Management
- Keys are encrypted at rest in local SQLite; supports plaintext copy, enable/disable, and per-channel multi-key management.
- Settings cover server address / port, theme, language, tray behavior, rate-limit & retry policy, and security audit toggles & mode.

### Logging & Observability
- `request_logs` records every request / response, token usage, and whether a retry occurred.
- Security finding details are viewable in the log detail view (including masked evidence and the forensic hash).

### Local-First
- All configuration, logs, and audit data never leave your machine; the key store lives at `%APPDATA%\com.wei.dongx\` (Windows).
- No external services or cloud dependencies.

## Architecture

A two-layer structure:

- **Management plane (Tauri invoke)**: the React frontend reads/writes config, keys, channels, rules, and logs through Tauri commands (17 commands).
- **Data plane (Axum)**: a standalone HTTP service listening on `127.0.0.1:9842` runs the OpenAI-compatible proxy pipeline (auth → dispatch → protocol conversion → forward → logging / audit).

```
┌────────────┐     Tauri invoke      ┌──────────────────┐
│  React UI  │ ───────────────────▶ │  Mgmt (Rust)     │
└────────────┘                      └──────────────────┘
       │  OpenAI-compatible HTTP
       ▼
┌──────────────────────────────────────────────┐
│  Axum data plane  (127.0.0.1:9842)             │
│  auth → dispatcher → adapter → upstream        │
│        └─ failover / circuit-breaker / audit   │
└──────────────────────────────────────────────┘
```

## Tech Stack

- Frontend: React 19 · TypeScript · Vite 7 · TailwindCSS 4 · shadcn/ui · lucide-react
- Backend: Rust · Tauri 2 · Axum · SQLite (sqlx) · reqwest

## Getting Started

```bash
# Install frontend dependencies
npm install

# Dev mode (starts both frontend and Rust backend)
cargo tauri dev

# Build desktop installers
cargo tauri build
```

> Runtime data lives at `%APPDATA%\com.wei.dongx\` (Windows), containing config, the SQLite database, and logs.

## Notes

- This project is for personal learning / demonstration, built to my own needs — not a commercial or commissioned project.
- The security audit rules and gateway logic all run locally; do not expose the gateway listen address to the public internet.
- Current build artifacts are unsigned. On Windows, SmartScreen may show an "unknown publisher" warning during install — click "Run anyway". On macOS, an unsigned app may refuse to open the first time; run `xattr -cr /Applications/DongX.app` in Terminal, then open it from Finder via right-click.

## License

[MIT](./LICENSE)
