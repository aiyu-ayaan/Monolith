//! `monolith` binary. Config from env (doc 03 §9; file config and CLI flags later):
//! `MONOLITH_DATA` (default `./monolith_data`), `MONOLITH_ADDR` (default `0.0.0.0:8090`),
//! `MONOLITH_ADMIN_EMAIL` + `MONOLITH_ADMIN_PASSWORD` create the first superuser.

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
    if reg.superuser_count().await? == 0 {
        match (
            std::env::var("MONOLITH_ADMIN_EMAIL"),
            std::env::var("MONOLITH_ADMIN_PASSWORD"),
        ) {
            (Ok(email), Ok(password)) => {
                reg.create_superuser(&email, &password).await?;
                tracing::info!(%email, "created first superuser");
            }
            _ => tracing::warn!(
                "no superuser yet: restart with MONOLITH_ADMIN_EMAIL and MONOLITH_ADMIN_PASSWORD set to create one"
            ),
        }
    }

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, data = %data.display(), "monolith listening");
    axum::serve(listener, monolith_server::router(reg))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
