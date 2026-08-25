//! Pure domain model for Sprout.
//!
//! The server only stores routing metadata and opaque encrypted payloads.  This
//! crate deliberately has no persistence or transport concerns.

pub mod agents;
pub mod attachments;
pub mod comments;
pub mod encrypted;
pub mod external_tools;
pub mod formal_release;
pub mod identity;
pub mod ids;
pub mod permissions;
pub mod presets;
pub mod questionnaire;
pub mod recurrence;
pub mod retention;
pub mod sync;
pub mod tasks;

pub use agents::*;
pub use attachments::*;
pub use comments::*;
pub use encrypted::*;
pub use external_tools::*;
pub use formal_release::*;
pub use identity::*;
pub use ids::*;
pub use permissions::*;
pub use presets::*;
pub use questionnaire::*;
pub use recurrence::*;
pub use retention::*;
pub use sync::*;
pub use tasks::*;
