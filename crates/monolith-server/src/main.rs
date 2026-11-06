//! `monolith` binary. Config from env (doc 03 §9; file config and CLI flags later):
//! `MONOLITH_DATA` (default `./monolith_data`), `MONOLITH_ADDR` (default `0.0.0.0:8090`).
//! With no superuser yet, start-up prints a one-time setup URL (doc 18 §2); credentials
//! never pass through env vars or config files.

use std::path::PathBuf;
use std::sync::Arc;

use monolith_core::Registry;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("MONOLITH_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let data =
        PathBuf::from(std::env::var("MONOLITH_DATA").unwrap_or_else(|_| "./monolith_data".into()));
    let addr = std::env::var("MONOLITH_ADDR").unwrap_or_else(|_| "0.0.0.0:8090".into());

    let reg = Arc::new(Registry::open(&data).await?);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, data = %data.display(), "monolith listening");
    if let Some(t) = reg.issue_install_token().await? {
        let local = listener.local_addr()?;
        let host = if local.ip().is_unspecified() {
            format!("127.0.0.1:{}", local.port())
        } else {
            local.to_string()
        };
        // Printed, not logged at a level filters can hide: this is the only way in.
        println!(
            "\n  No superuser yet. Open this one-time link within 1 hour to create one:\n\n  http://{host}/_/#/install/{t}\n\n  (Restart the server for a new link.)\n"
        );
    }
    axum::serve(listener, monolith_server::router(reg))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
