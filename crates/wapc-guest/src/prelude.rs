//! Glob imports for common guest module development

#[cfg(feature = "codec")]
pub use wapc_codec::messagepack;

pub use crate::protocol::{console_log, host_call, register_function, CallResult, HandlerResult};

/// Utility type for a Pin<Box<Future<T>>>
pub type BoxedFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'static>>;
