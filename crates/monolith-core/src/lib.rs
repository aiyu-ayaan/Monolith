//! Apps, collections and records above the `AppStore` seam (doc 03 §2, doc 05).

pub mod apikeys;
pub mod app;
pub mod auth_config;
pub mod backup;
pub mod codec;
pub mod ddl;
pub mod debug;
pub mod error;
pub mod files;
pub mod list;
pub mod metrics;
pub mod realtime;
pub mod schema;
pub mod system;
pub mod thumbs;

pub use apikeys::{ApiKey, ApiKeyIn, ApiKeyInfo};
pub use app::{
    App, BatchItem, BatchKind, ListParams, Principal, ReadOpts, RecordOut, TotalMode, WriteOpts,
};
pub use auth_config::{AuthConfigIn, AuthConfigView};
pub use backup::BackupInfo;
pub use debug::{ExplainQueryReq, ExplainQueryResp, ExplainRuleReq, ExplainRuleResp, RequestLog};
pub use error::{CoreError, CoreResult, FieldError};
pub use files::{BucketIn, BucketInfo, Files, ObjectInfo, ObjectPage, Upload};
pub use metrics::{Metrics, ReqKind};
pub use monolith_rules::Auth;
pub use monolith_store::Value;
pub use schema::{Collection, CollectionDef, Schema};
pub use system::{AppInfo, Registry};
