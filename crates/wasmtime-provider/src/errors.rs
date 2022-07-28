/// This crate's Error type
#[derive(thiserror::Error, Debug)]
pub enum Error {
  /// WASMTime initialization failed
  #[error("Initialization failed: {0}")]
  InitializationFailed(Box<dyn std::error::Error + Send + Sync>),
  /// The guest call function was not exported by the guest.
  #[error("WaPC init function (wapc_init) not exported by wasm module.")]
  InitNotFound,
  /// The guest call function was not exported by the guest.
  #[error("Guest call function (__guest_call) not exported by wasm module.")]
  GuestCallNotFound,
  /// The guest call function was not exported by the guest.
  #[error("Async guest call function (__async_guest_call) not exported by wasm module.")]
  AsyncGuestCallNotFound,
  /// The host response ready function was not exported by the guest.
  #[error("Host response ready function (__host_response_ready) not exported by wasm module.")]
  HostResponseReadyNotFound,
  /// Error originating from [wasi_common]
  #[error("{0}")]
  WasiError(#[from] wasi_common::Error),
  /// Error originating from [wasi_common]
  #[error("Error exposing directories with WASI: {0}")]
  PreopenedDirs(String),
}

impl From<Error> for wapc::errors::Error {
  fn from(e: Error) -> Self {
    wapc::errors::Error::ProviderFailure(Box::new(e))
  }
}
