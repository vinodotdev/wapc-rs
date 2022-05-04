use std::str::FromStr;
use strum::EnumIter;
pub use strum::IntoEnumIterator;

/// Functions called by guest, exported by host
pub mod host_exports {
  /// The waPC protocol function `__console_log`
  pub const HOST_CONSOLE_LOG: &str = "__console_log";
  /// The waPC protocol function `__host_call`
  pub const HOST_CALL: &str = "__host_call";
  /// The waPC protocol function `__host_call`
  pub const ASYNC_HOST_CALL: &str = "__async_host_call";
  /// The waPC protocol function `__guest_request`
  pub const GUEST_REQUEST_FN: &str = "__guest_request";
  /// The waPC protocol function `__host_response`
  pub const HOST_RESPONSE_FN: &str = "__host_response";
  /// The waPC protocol function `__host_response_len`
  pub const HOST_RESPONSE_LEN_FN: &str = "__host_response_len";
  /// The waPC protocol function `__guest_response`
  pub const GUEST_RESPONSE_FN: &str = "__guest_response";
  /// The waPC protocol function `__guest_error`
  pub const GUEST_ERROR_FN: &str = "__guest_error";
  /// The waPC protocol function `__host_error`
  pub const HOST_ERROR_FN: &str = "__host_error";
  /// The waPC protocol function `__host_error_len`
  pub const HOST_ERROR_LEN_FN: &str = "__host_error_len";
  /// The waPC protocol function `__async_guest_call_response`
  pub const GUEST_CALL_RESPONSE_READY: &str = "__guest_call_response_ready";
}

/// The exported host functions as an enum.
#[derive(Debug, Copy, Clone, EnumIter)]
#[allow(missing_docs)]
pub enum HostExports {
  ConsoleLog,
  HostCall,
  AsyncHostCall,
  GuestRequest,
  HostResponse,
  HostResponseLen,
  GuestResponse,
  GuestError,
  HostError,
  HostErrorLen,
  GuestResponseReady,
}

impl FromStr for HostExports {
  type Err = ();

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    let result = match s {
      host_exports::HOST_CONSOLE_LOG => Self::ConsoleLog,
      host_exports::HOST_CALL => Self::HostCall,
      host_exports::ASYNC_HOST_CALL => Self::AsyncHostCall,
      host_exports::GUEST_REQUEST_FN => Self::GuestRequest,
      host_exports::HOST_RESPONSE_FN => Self::HostResponse,
      host_exports::HOST_RESPONSE_LEN_FN => Self::HostResponseLen,
      host_exports::GUEST_RESPONSE_FN => Self::GuestResponse,
      host_exports::GUEST_ERROR_FN => Self::GuestError,
      host_exports::HOST_ERROR_FN => Self::HostError,
      host_exports::HOST_ERROR_LEN_FN => Self::HostErrorLen,
      host_exports::GUEST_CALL_RESPONSE_READY => Self::GuestResponseReady,
      _ => return Err(()),
    };
    Ok(result)
  }
}

impl AsRef<str> for HostExports {
  fn as_ref(&self) -> &str {
    match self {
      Self::ConsoleLog => host_exports::HOST_CONSOLE_LOG,
      Self::HostCall => host_exports::HOST_CALL,
      Self::AsyncHostCall => host_exports::ASYNC_HOST_CALL,
      Self::GuestRequest => host_exports::GUEST_REQUEST_FN,
      Self::HostResponse => host_exports::HOST_RESPONSE_FN,
      Self::HostResponseLen => host_exports::HOST_RESPONSE_LEN_FN,
      Self::GuestResponse => host_exports::GUEST_RESPONSE_FN,
      Self::GuestError => host_exports::GUEST_ERROR_FN,
      Self::HostError => host_exports::HOST_ERROR_FN,
      Self::HostErrorLen => host_exports::HOST_ERROR_LEN_FN,
      Self::GuestResponseReady => host_exports::GUEST_CALL_RESPONSE_READY,
    }
  }
}

impl std::fmt::Display for HostExports {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.as_ref())
  }
}

/// Functions called by host, exported by guest
pub mod guest_exports {
  /// The waPC protocol function `__guest_call`
  pub const GUEST_CALL: &str = "__guest_call";

  /// The waPC protocol function `__guest_call`
  pub const ASYNC_GUEST_CALL: &str = "__async_guest_call";

  /// The waPC protocol function `__async_host_call_response`
  pub const HOST_CALL_RESPONSE_READY: &str = "__host_call_response_ready";

  /// The waPC protocol function `wapc_init`
  pub const WAPC_INIT: &str = "wapc_init";

  /// The waPC protocol function `_start`
  pub const TINYGO_START: &str = "_start";

  /// Start functions to attempt to call - order is important
  pub const REQUIRED_STARTS: [&str; 2] = [TINYGO_START, WAPC_INIT];
}

/// The exported guest functions as an enum.
#[derive(Debug, Copy, Clone, EnumIter)]
#[allow(missing_docs)]
pub enum GuestExports {
  Init,
  Start,
  GuestCall,
  AsyncGuestCall,
  HostResponseReady,
}

impl FromStr for GuestExports {
  type Err = ();

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    let result = match s {
      guest_exports::WAPC_INIT => Self::Init,
      guest_exports::TINYGO_START => Self::Start,
      guest_exports::HOST_CALL_RESPONSE_READY => Self::HostResponseReady,
      guest_exports::GUEST_CALL => Self::GuestCall,
      guest_exports::ASYNC_GUEST_CALL => Self::AsyncGuestCall,

      _ => return Err(()),
    };
    Ok(result)
  }
}

impl AsRef<str> for GuestExports {
  fn as_ref(&self) -> &str {
    match self {
      GuestExports::Init => guest_exports::WAPC_INIT,
      GuestExports::Start => guest_exports::TINYGO_START,
      GuestExports::GuestCall => guest_exports::GUEST_CALL,
      GuestExports::AsyncGuestCall => guest_exports::ASYNC_GUEST_CALL,
      GuestExports::HostResponseReady => guest_exports::HOST_CALL_RESPONSE_READY,
    }
  }
}

impl std::fmt::Display for GuestExports {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.as_ref())
  }
}
