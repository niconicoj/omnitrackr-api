use std::{path::Path, sync::Arc};

use axum::{Router, extract::State, routing::post};
use piper_rs::synth::PiperSpeechSynthesizer;
use serde::Deserialize;
use tokio::net::TcpListener;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing::{info, instrument};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use validation::ValidatedJson;
use validator::Validate;

mod error;
mod validation;

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
    synth_permits: tokio::sync::Semaphore,
    synthesizer: PiperSpeechSynthesizer,
}

impl AppState {
    fn init() -> Self {
        let config_path = "en_US-hfc_female-medium.onnx.json";
        let piper_model = piper_rs::from_config_path(Path::new(config_path)).unwrap();
        let synthesizer = PiperSpeechSynthesizer::new(piper_model).unwrap();
        Self {
            synthesizer,
            synth_permits: tokio::sync::Semaphore::new(1),
        }
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
    let mut bytes: Vec<u8> = Vec::new();
    let permit = state.synth_permits.acquire().await.unwrap();
    state
        .synthesizer
        .synthesize_to_buffer(std::io::Cursor::new(&mut bytes), input.text, None)
        .unwrap();
    drop(permit);
    bytes
}
