# omnitrackr-api

HTTP API server for text-to-speech synthesis using [Piper](https://github.com/rhasspy/piper). Built with Axum and Tokio, deployed to Google Cloud Run.

## Prerequisites

- Rust (stable 1.85+)
- System packages: `libclang-dev`, `g++`, `cmake`, `git`, `wget`
- `espeak-ng-data` (required at runtime by Piper)

On Debian/Ubuntu:

```bash
apt-get install libclang-dev g++ cmake git wget espeak-ng-data
```

## Setup

Download the ONNX voice model files before building or running:

```bash
make download-assets
```

This fetches `en_US-hfc_female-medium.onnx` and `en_US-hfc_female-medium.onnx.json` from HuggingFace into the project root. The server expects these files in the working directory at startup.

## Build

```bash
cargo build            # debug build
cargo build --release  # release build
```

## Run

```bash
cargo run
```

The server starts on `0.0.0.0:3000`. Set `RUST_LOG` to control log verbosity (defaults to `debug` for this crate and `tower_http`):

```bash
RUST_LOG=info cargo run
```

## API

### `POST /synthesize`

Synthesizes the given text and returns a WAV audio file.

**Request body:**

```json
{ "text": "Hello, world!" }
```

- `text` — required, 1–512 characters.

**Response:** raw WAV audio bytes (`Content-Type: application/octet-stream`).

**Example using the helper script** (server must be running):

```bash
./scripts/synthesize.sh "Hello, world!"
# saves response to output.wav
```

Or with curl directly:

```bash
curl -X POST http://127.0.0.1:3000/synthesize \
  -H "Content-Type: application/json" \
  -d '{"text": "Hello, world!"}' \
  --output output.wav
```

## Test

```bash
cargo test                         # run all tests
cargo test <test_name>             # run a single test (substring match)
cargo test <test_name> --nocapture # with stdout/stderr output
```

## Lint and Format

```bash
cargo fmt              # auto-format all source files
cargo fmt --check      # check formatting without writing (for CI)
cargo clippy           # run Clippy lints
cargo clippy -- -D warnings  # fail on any warning
```

## Docker

Build the image (downloads model files during the build):

```bash
docker build -t omnitrackr-api .
```

Run the container:

```bash
docker run -p 3000:3000 omnitrackr-api
```

The Dockerfile uses a multi-stage build: `rust:1.85.0-slim` as the builder and `debian:stable-slim` as the runtime image.
