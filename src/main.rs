use std::{
    io::Cursor,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

use axum::{Router, extract::State, routing::post};
use piper_rs::{CorePiperModel, synth::PiperSpeechSynthesizer};
use riff_wave::WaveWriter;
use serde::Deserialize;
use tokio::net::TcpListener;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing::{info, instrument};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use validation::ValidatedJson;
use validator::Validate;

mod error;
mod validation;

/// espeak-ng operates entirely on process-global C state with no internal locking in synchronous
/// (retrieval) mode. Concurrent calls to `phonemize_text` therefore race on that global state and
/// cause segfaults. This mutex serialises the phonemization step across all requests.
/// ONNX inference (`speak_one_sentence`) is safe to run concurrently and is not covered by this
/// lock.
static ESPEAK_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

fn espeak_mutex() -> &'static Mutex<()> {
    ESPEAK_MUTEX.get_or_init(|| Mutex::new(()))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!(
                    "{}=debug,tower_http=debug,axum::rejection=trace",
                    env!("CARGO_CRATE_NAME")
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = Arc::new(AppState::init());

    let app = Router::new()
        .route("/synthesize", post(synthesize))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(true)),
        )
        .with_state(state);

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

struct AppState {
    synthesizer: PiperSpeechSynthesizer,
}

impl AppState {
    fn init() -> Self {
        let config_path = "en_US-hfc_female-medium.onnx.json";
        let piper_model = piper_rs::from_config_path(Path::new(config_path)).unwrap();
        let synthesizer = PiperSpeechSynthesizer::new(piper_model).unwrap();
        Self { synthesizer }
    }
}

#[derive(Debug, Deserialize, Validate)]
struct SynthesizeRequest {
    #[validate(length(min = 1, max = 512, message = "Must be between 1 and 512 characters"))]
    text: String,
}

#[instrument(skip(state))]
async fn synthesize(
    State(state): State<Arc<AppState>>,
    ValidatedJson(input): validation::ValidatedJson<SynthesizeRequest>,
) -> Vec<u8> {
    info!("Synthesizing text: {:?}", input.text);

    // Phonemization is the only step that touches espeak-ng's global C state.
    // Acquire the mutex, phonemize, then immediately release it so that ONNX
    // inference (the slow part) runs concurrently across requests.
    let phonemes = {
        let _guard = espeak_mutex().lock().unwrap();
        state.synthesizer.phonemize_text(&input.text).unwrap()
    };

    // ONNX inference is safe to run concurrently on the CPU execution provider.
    let audio_chunks: Vec<_> = phonemes
        .to_vec()
        .into_iter()
        .map(|sentence| state.synthesizer.speak_one_sentence(sentence).unwrap())
        .collect();

    let audio_info = state.synthesizer.audio_output_info().unwrap();
    let i16_samples: Vec<i16> = audio_chunks
        .into_iter()
        .flat_map(|audio| audio.samples.to_i16_vec())
        .collect();

    let mut bytes: Vec<u8> = Vec::new();
    let mut writer = WaveWriter::new(
        audio_info.num_channels as u16,
        audio_info.sample_rate as u32,
        (audio_info.sample_width * 8) as u16,
        Cursor::new(&mut bytes),
    )
    .unwrap();
    for sample in &i16_samples {
        writer.write_sample_i16(*sample).unwrap();
    }
    writer.sync_header().unwrap();
    drop(writer);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use serde_json::json;
    use tower::ServiceExt;

    fn build_app() -> Router {
        let state = Arc::new(AppState::init());
        Router::new()
            .route("/synthesize", post(synthesize))
            .with_state(state)
    }

    fn json_request(body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/synthesize")
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn synthesize_valid_text_returns_wav_bytes() {
        let app = build_app();
        let response = app
            .oneshot(json_request(json!({"text": "Hello world"})))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(!body.is_empty(), "expected non-empty WAV bytes");
        // WAV files start with the RIFF header magic bytes
        assert_eq!(&body[..4], b"RIFF", "expected a WAV RIFF header");
    }

    #[tokio::test]
    async fn synthesize_empty_text_returns_400() {
        let app = build_app();
        let response = app
            .oneshot(json_request(json!({"text": ""})))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn synthesize_missing_text_field_returns_400() {
        let app = build_app();
        let response = app.oneshot(json_request(json!({}))).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// Regression test: concurrent calls to synthesize must not segfault.
    ///
    /// espeak-ng operates on process-global C state with no internal locking.
    /// Previously, concurrent requests would race on that state and segfault.
    /// The fix serialises only the phonemization step via `ESPEAK_MUTEX`, leaving
    /// ONNX inference (the slow part) free to run concurrently.
    #[tokio::test(flavor = "multi_thread")]
    async fn concurrent_synthesis_does_not_segfault() {
        let state = Arc::new(AppState::init());
        let concurrency = 8;

        let handles: Vec<_> = (0..concurrency)
            .map(|i| {
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    let phonemes = {
                        let _guard = espeak_mutex().lock().unwrap();
                        state
                            .synthesizer
                            .phonemize_text(&format!("Concurrent synthesis request number {i}"))
                            .unwrap()
                    };
                    let samples: Vec<i16> = phonemes
                        .to_vec()
                        .into_iter()
                        .flat_map(|sentence| {
                            state
                                .synthesizer
                                .speak_one_sentence(sentence)
                                .unwrap()
                                .samples
                                .to_i16_vec()
                        })
                        .collect();
                    assert!(!samples.is_empty());
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap();
        }
    }
}
