mod api;
mod bootstrap;
mod ontology_fallback;
mod state;

use std::env;

#[tokio::main]
async fn main() {
    let state = bootstrap::build_state().expect("initialize VoiceOS Rust gateway");
    let address = env::var("VOICEOS_RUST_LISTEN").unwrap_or_else(|_| "127.0.0.1:8790".to_owned());
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .expect("bind Rust gateway");

    println!("VoiceOS Rust gateway listening on http://{address}");
    axum::serve(listener, api::router(state))
        .await
        .expect("serve Rust gateway");
}
