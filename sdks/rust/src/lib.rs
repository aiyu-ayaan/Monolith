//! Official Rust client SDK for Monolith.
//!
//! Monolith is a backend suite providing database access rules, authentication,
//! realtime subscriptions, offline sync, file storage, analytics, and backups.
//!
//! # Getting Started
//!
//! ```no_run
//! use monolith_client::{Client, ListOptions, Record};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = Client::builder("http://127.0.0.1:8090")
//!         .app("default")
//!         .build()?;
//!
//!     // Authenticate
//!     client.auth().sign_in_with_password("user@example.com", "secret").await?;
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

pub use auth::{AuthService, AuthSession};
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
