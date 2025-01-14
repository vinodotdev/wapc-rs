#![deny(
  clippy::expect_used,
  clippy::explicit_deref_methods,
  clippy::option_if_let_else,
  clippy::await_holding_lock,
  clippy::cloned_instead_of_copied,
  clippy::explicit_into_iter_loop,
  clippy::flat_map_option,
  clippy::fn_params_excessive_bools,
  clippy::implicit_clone,
  clippy::inefficient_to_string,
  clippy::large_types_passed_by_value,
  clippy::manual_ok_or,
  clippy::map_flatten,
  clippy::map_unwrap_or,
  clippy::must_use_candidate,
  clippy::needless_for_each,
  clippy::needless_pass_by_value,
  clippy::option_option,
  clippy::redundant_else,
  clippy::semicolon_if_nothing_returned,
  clippy::too_many_lines,
  clippy::trivially_copy_pass_by_ref,
  clippy::unnested_or_patterns,
  clippy::future_not_send,
  clippy::useless_let_if_seq,
  clippy::str_to_string,
  clippy::inherent_to_string,
  clippy::let_and_return,
  clippy::string_to_string,
  clippy::try_err,
  clippy::unused_async,
  clippy::missing_enforced_import_renames,
  clippy::nonstandard_macro_braces,
  clippy::rc_mutex,
  clippy::unwrap_or_else_default,
  clippy::manual_split_once,
  clippy::derivable_impls,
  clippy::needless_option_as_deref,
  clippy::iter_not_returning_iterator,
  clippy::same_name_method,
  clippy::manual_assert,
  clippy::non_send_fields_in_send_ty,
  clippy::equatable_if_let,
  bad_style,
  clashing_extern_declarations,
  const_err,
  dead_code,
  deprecated,
  explicit_outlives_requirements,
  improper_ctypes,
  invalid_value,
  missing_copy_implementations,
  missing_debug_implementations,
  mutable_transmutes,
  no_mangle_generic_items,
  non_shorthand_field_patterns,
  overflowing_literals,
  path_statements,
  patterns_in_fns_without_body,
  private_in_public,
  trivial_bounds,
  trivial_casts,
  trivial_numeric_casts,
  type_alias_bounds,
  unconditional_recursion,
  unreachable_pub,
  unsafe_code,
  unstable_features,
  unused,
  unused_allocation,
  unused_comparisons,
  unused_import_braces,
  unused_parens,
  unused_qualifications,
  while_true,
  missing_docs
)]
#![doc(html_logo_url = "https://avatars0.githubusercontent.com/u/54989751?s=200&v=4")]
#![doc = include_str!("../README.md")]

#[macro_use]
extern crate tracing;

pub mod errors;

use std::error::Error;

/// The host module name / namespace that guest modules must use for imports
pub const HOST_NAMESPACE: &str = "wapc";

/// A list of the function names that are part of each waPC conversation
pub mod wapc_functions;

mod wapchost;
mod wasi;

use futures_core::future::BoxFuture;
pub use wapc_functions::*;
pub use wapchost::modulestate::ModuleState;
pub use wapchost::traits::{ModuleHost, ProviderCallContext, WebAssemblyEngineProvider};
pub use wapchost::{WapcCallContext, WapcHost, WapcHostBuilder};
pub use wasi::WasiParams;

/// The signature of a Host Callback function.
pub type AsyncHostCallback =
  dyn Fn(i32, String, String, String, Vec<u8>) -> BoxFuture<'static, HostResult> + Sync + Send + 'static;

/// The signature of a Host Callback function.
pub type HostCallback = dyn Fn(i32, String, String, String, Vec<u8>) -> HostResult + Sync + Send + 'static;

/// The result type that hostcalls produce.
pub type HostResult = Result<Vec<u8>, Box<dyn Error + Send + Sync>>;

/// The signature of a Host response ready callback.
pub type HostResponseReady = dyn Fn(i32, i32) -> BoxFuture<'static, ()> + Sync + Send + 'static;

#[derive(Debug, Clone)]
/// Represents a waPC invocation, which is a combination of an operation string and the
/// corresponding binary payload
pub struct Invocation {
  /// The waPC command to execute.
  pub operation: String,
  /// The payload to send.
  pub msg: Vec<u8>,
}

impl Invocation {
  /// Creates a new invocation
  fn new<T: AsRef<str>>(operation: T, msg: Vec<u8>) -> Invocation {
    Invocation {
      operation: operation.as_ref().to_owned(),
      msg,
    }
  }
}
