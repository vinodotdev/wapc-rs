//! Glob imports for common guest module development

#[cfg(feature = "codec")]
pub use wapc_codec::messagepack;

pub use crate::protocol::{console_log, host_call, register_function, CallResult, HandlerResult};
