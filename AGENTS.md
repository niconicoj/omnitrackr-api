# AGENTS.md

This file provides guidance for AI coding agents working in this repository.

## Project Overview

`omnitrackr-api` is a Rust HTTP API server built with Axum that performs text-to-speech synthesis
using the `piper-rs` library. It runs on Axum 0.8 with Tokio, uses ONNX model files at runtime,
and is deployed to Google Cloud Run via Docker.

- **Language**: Rust (edition 2024)
- **Runtime**: Tokio (async, full features)
- **HTTP framework**: Axum 0.8
- **Active toolchain**: nightly (local), `rust:1.85.0-slim` in Docker — keep code compatible with
  stable 1.85+

---

## Build, Run, and Tooling Commands

```bash
# Build
cargo build               # debug build
cargo build --release     # release build

# Run
cargo run                 # start the server on 0.0.0.0:3000

# Format
cargo fmt                 # format all source files
cargo fmt --check         # check formatting without writing (use in CI)

# Lint
cargo clippy              # run Clippy with default lints
cargo clippy -- -D warnings   # treat all warnings as errors (preferred for CI)

# Test
cargo test                # run all tests
cargo test <test_name>    # run a single test by name (substring match)
cargo test -- --nocapture # run tests with stdout/stderr visible

# Assets (ONNX model files, required at runtime)
make download-assets      # download .onnx and .onnx.json model files

# Manual smoke test (server must be running)
./scripts/synthesize.sh "Hello world"   # POST to /synthesize, saves output.wav
```

No `rustfmt.toml`, `.clippy.toml`, or `rust-toolchain.toml` are present — all tools use their
defaults.

---

## Project Structure

```
src/
  main.rs         # App entry point: AppState, router setup, request structs, handlers
  error.rs        # ServerError enum (thiserror) + IntoResponse impl
  validation.rs   # ValidatedJson<T> custom Axum extractor
scripts/
  synthesize.sh   # Dev helper for manual API testing
Makefile          # download-assets target
Dockerfile        # Multi-stage: rust:1.85.0-slim builder → debian:stable-slim runtime
.github/
  workflows/
    main.yml      # CI/CD: Docker build + deploy to Cloud Run on push to main
```

---

## Code Style Guidelines

### Imports

- Place `std::` imports first, then external crate imports (alphabetically), then local `use`
  items last.
- Use `{}` grouping to combine multiple items from the same crate/module on one line:
  ```rust
  use axum::{Router, extract::State, routing::post};
  use std::sync::Arc;
  ```
- Avoid glob imports (`use foo::*`) except in test modules where `use super::*` is acceptable.

### Formatting

- Use `cargo fmt` (rustfmt defaults) — no custom config file.
- 4-space indentation (rustfmt enforced).
- Keep lines within 100 characters where practical; rustfmt will enforce its own limits.

### Types and Naming

- `PascalCase` for all types, structs, enums, and traits.
- `snake_case` for functions, methods, variables, fields, and module/file names.
- `SCREAMING_SNAKE_CASE` for constants and statics.
- Use descriptive names; avoid single-letter names except in short closures (`|x| x + 1`) or
  iterator chains.
- Newtype wrappers are preferred for domain distinction (e.g., `ValidatedJson<T>`).

### Structs and Derives

- Request/response structs should derive `Debug, Deserialize` (and `Serialize` when needed).
- Structs requiring validation should also derive `Validate` from the `validator` crate.
- Use field-level `#[validate(...)]` attributes with explicit `message =` strings for clarity:
  ```rust
  #[validate(length(min = 1, max = 500, message = "text must be between 1 and 500 characters"))]
  pub text: String,
  ```

### Error Handling

- Define errors using `thiserror` — `#[derive(Debug, Error)]` enums in `src/error.rs`.
- Use `#[error(transparent)]` with `#[from]` for automatic error conversion from upstream types.
- Implement `IntoResponse` on the error enum to map variants to appropriate HTTP status codes:
  ```rust
  impl IntoResponse for ServerError {
      fn into_response(self) -> Response {
          match self {
              ServerError::ValidationError(e) => {
                  (StatusCode::BAD_REQUEST, format!("{e}")).into_response()
              }
              // ...
          }
      }
  }
  ```
- Use the `?` operator for propagation throughout handler and extractor code.
- `.unwrap()` is acceptable only at program startup (initialization, one-time setup) or for values
  that are provably infallible. Avoid `.unwrap()` in request-handling paths.
- Never use `.expect()` in production paths; prefer proper error variants or `.unwrap()` with a
  comment explaining why it is safe.

### Axum Handlers

- Handler signatures use named destructuring for extractors:
  ```rust
  async fn handle(
      State(state): State<Arc<AppState>>,
      ValidatedJson(payload): ValidatedJson<MyRequest>,
  ) -> Result<impl IntoResponse, ServerError> { ... }
  ```
- Instrument handlers with `#[instrument(skip(state))]` (or skip individual large fields) to
  avoid logging sensitive or bulky data.
- Use `Arc<AppState>` for shared state passed via `State(...)`.
- For concurrency-limited resources, use `tokio::sync::Semaphore`; acquire with `.await.unwrap()`
  (infallible after initialization) and `drop(permit)` explicitly when done.

### Async and Concurrency

- All async code runs on the Tokio multi-thread runtime (`#[tokio::main]`).
- Prefer structured concurrency; avoid unbounded task spawning without backpressure.
- Use `Arc<T>` for shared ownership across tasks; `Mutex`/`RwLock` only when mutation is needed.

### Tracing and Logging

- Initialize tracing in `main` via `tracing_subscriber::registry()` with `EnvFilter`.
- Default filter: `{crate_name}=debug,tower_http=debug,axum::rejection=trace`.
- Use `tracing::{debug, info, warn, error}` macros (not `println!`) in all non-test code.
- Use `#[instrument]` on public async functions and handlers.

### Custom Extractors

- Follow the `ValidatedJson<T>` pattern in `src/validation.rs` for new extractors:
  - Newtype struct with `pub` inner field.
  - `impl<T, S> FromRequest<S> for MyExtractor<T>` with appropriate trait bounds.
  - `type Rejection = ServerError` to unify error handling.

### Adding New Routes

- Define request/response structs in the same file as the handler, or in a dedicated module if
  they grow large.
- Register routes in the `Router::new()` chain in `main.rs`.
- Keep handler functions focused; extract business logic into separate functions or modules.

---

## Testing

No tests exist yet. When adding tests:

- Unit tests go in a `#[cfg(test)]` module at the bottom of the relevant source file.
- Integration tests go in a `tests/` directory at the project root.
- Use `axum::test` helpers or `axum_test` crate for handler integration tests.
- Run a specific test:
  ```bash
  cargo test my_test_name
  cargo test my_test_name -- --nocapture   # with output
  ```

---

## Docker and Deployment

- The Dockerfile is a multi-stage build; the builder stage downloads ONNX model files during
  the image build (requires internet access or pre-cached layers).
- CI (`main.yml`) builds and pushes a Docker image to Google Artifact Registry and deploys to
  Cloud Run (`europe-west9`) on every push to `main`.
- The runtime binary expects model files adjacent to it and the `PIPER_ESPEAKNG_DATA_DIRECTORY`
  env var set to the espeak-ng data path.
- Port `3000` is hardcoded; ensure Cloud Run routes to that port.

---

## Key Dependencies

| Crate | Purpose |
|---|---|
| `axum 0.8` | HTTP routing and extractors |
| `tokio` (full) | Async runtime |
| `tower-http` | Middleware (tracing layer) |
| `serde` (derive) | Serialization/deserialization |
| `thiserror` | Error type derivation |
| `validator` (derive) | Request payload validation |
| `tracing` + `tracing-subscriber` | Structured logging |
| `piper-rs` (git) | Text-to-speech synthesis via ONNX |
