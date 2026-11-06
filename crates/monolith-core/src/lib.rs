//! Apps, collections and records above the `AppStore` seam (doc 03 §2, doc 05).

pub mod app;
pub mod codec;
pub mod ddl;
pub mod error;
pub mod list;
pub mod schema;
pub mod system;

pub use app::{App, BatchItem, BatchKind, ListParams, Principal};
pub use error::{CoreError, CoreResult, FieldError};
pub use schema::{Collection, CollectionDef, Schema};
pub use system::{AppInfo, Registry};
