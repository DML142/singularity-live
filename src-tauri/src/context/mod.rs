mod loader;
mod manifest;
mod prompt;
mod selection;
mod session;

pub use loader::{ContextError, ContextPack, ContextPackLoader};
pub use prompt::{build_session_system_prompt, build_system_prompt};
pub use selection::select_context;
pub(crate) use session::{
    MAX_RETAINED_ASSISTANT_BYTES, MAX_SESSION_SUMMARY_BYTES, truncate_to_bytes,
};
pub use session::{SessionHistory, SessionTurn, SummaryBatch};
