//! Official Rust client SDK for Monolith.
//!
//! Monolith is a backend suite providing database access rules, authentication,
//! realtime subscriptions, offline sync, file storage, analytics, and backups.
//!
//! # Getting Started
//!
//! ```no_run
//! use monolith_client::Monolith;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mono = Monolith::init("https://api.example.com", "shop")?; // ready client, auto token refresh
//!     let tasks = mono.records("tasks").list(None).await?;           // records
//!     println!("{}", tasks.items.len());
//!     Ok(())
//! }
//! ```
//!
//! # Full example
//!
//! ```no_run
//! use monolith_client::{Client, ListOptions, Record};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = Client::builder("http://127.0.0.1:8090")
//!         .app("default")
//!         .session_file("session.json") // optional: keep the session between runs
//!         .build()?;
//!
//!     // Authenticate an app user (superusers: `client.auth_collection("_superusers")`)
//!     client.auth().sign_in("user@example.com", "secret").await?;
//!
//!     // List records
//!     let tasks = client.records("tasks").list(None).await?;
//!     println!("Found {} tasks", tasks.items.len());
//!
//!     Ok(())
//! }
//! ```

pub mod auth;
pub mod client;
pub mod collections;
pub mod errors;
pub mod files;
pub mod realtime;

pub use auth::{
    AuthService, AuthSession, AuthState, EmailPasswordConfig, FileSessionStore, MemorySessionStore,
    ProviderInfo, PublicAuth, SessionStore,
};
/// Alias for [`AuthService`].
pub type Auth = AuthService;

pub use client::{BatchOp, BatchResponse, BatchResultItem, Client, ClientBuilder};
pub use collections::{Collection, CountResult, ListOptions, ListResult, Record};
pub use errors::{Error, FieldProblem, Problem, Result};
pub use files::{FilesService, ObjectInfo, ObjectPage};
pub use realtime::{
    ChangeType, DocumentChange, QuerySnapshot, RealtimeAction, RealtimeEvent, SnapshotOptions,
    SubscribeOptions, Subscription,
};

/// One-line start: `let mono = Monolith::init(url, app)?;`
pub struct Monolith;

impl Monolith {
    /// A ready [`Client`] for `app` with an in-memory session and automatic token refresh.
    /// For a persistent session use [`Monolith::builder`] with `.session_file(path)`.
    pub fn init(url: impl Into<String>, app: impl Into<String>) -> Result<Client> {
        Self::builder(url, app).build()
    }

    pub fn builder(url: impl Into<String>, app: impl Into<String>) -> ClientBuilder {
        Client::builder(url).app(app)
    }
}
