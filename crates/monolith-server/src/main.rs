//! `monolith` binary. Config from env (doc 03 §9; file config and CLI flags later):
//! `MONOLITH_DATA` (default `./monolith_data`), `MONOLITH_ADDR` (default `0.0.0.0:8090`).
//! With no superuser yet, start-up prints a one-time setup URL (doc 18 §2); credentials
//! never pass through env vars or config files.
//!
//! Commands: `monolith [serve]` runs the server; `monolith superuser create <email>`
//! adds a superuser for headless setup (`docker exec -it`). The password is read from
//! the terminal without echo, or as one line from piped stdin; never from argv.

use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use monolith_core::{CoreError, Registry};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: monolith [serve]\n       monolith superuser create <email>";

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let data =
        PathBuf::from(std::env::var("MONOLITH_DATA").unwrap_or_else(|_| "./monolith_data".into()));
    let res = match args[..] {
        [] | ["serve"] => serve(data).await,
        ["superuser", "create", email] => superuser_create(data, email).await,
        ["-h" | "--help" | "help"] => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(data: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("MONOLITH_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
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

/// Safe next to a running server: SQLite WAL + `busy_timeout` serialize the two writers,
/// and the server's install link stops working once any superuser exists.
async fn superuser_create(data: PathBuf, email: &str) -> Result<(), Box<dyn std::error::Error>> {
    let password = if std::io::stdin().is_terminal() {
        let p = rpassword::prompt_password("Password: ")?;
        if rpassword::prompt_password("Repeat password: ")? != p {
            return Err("passwords do not match".into());
        }
        p
    } else {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        line.trim_end_matches(['\r', '\n']).to_owned()
    };
    let reg = Registry::open(&data).await?;
    match reg.create_superuser(email, &password).await {
        Ok(_) => {
            println!("superuser {} created", email.trim().to_ascii_lowercase());
            Ok(())
        }
        Err(CoreError::Validation(errs)) => Err(errs
            .iter()
            .map(|e| format!("{}: {}", e.field, e.message))
            .collect::<Vec<_>>()
            .join("; ")
            .into()),
        Err(e) => Err(e.into()),
    }
}
