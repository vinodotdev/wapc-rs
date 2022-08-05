pub(crate) mod modulestate;
pub(crate) mod traits;

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use self::modulestate::{CallStatus, ModuleState};
use self::traits::WebAssemblyEngineProvider;
use crate::{errors, AsyncHostCallback, HostCallback, Invocation, ProviderCallContext};

static GLOBAL_MODULE_COUNT: AtomicU64 = AtomicU64::new(1);
static GLOBAL_CONTEXT_COUNT: AtomicU64 = AtomicU64::new(1);

type Result<T> = std::result::Result<T, crate::errors::Error>;

/// A WebAssembly host runtime for waPC-compliant modules
///
/// Use an instance of this struct to provide a means of invoking procedure calls by
/// specifying an operation name and a set of bytes representing the opaque operation payload.
/// `WapcHost` makes no assumptions about the contents or format of either the payload or the
/// operation name, other than that the operation name is a UTF-8 encoded string.
#[must_use]
#[allow(missing_debug_implementations)]
pub struct WapcHost {
  engine: RefCell<Box<dyn WebAssemblyEngineProvider>>,
  async_hostcall: Option<Arc<AsyncHostCallback>>,
  sync_hostcall: Option<Arc<HostCallback>>,
  id: u64,
}

impl WapcHost {
  /// Manually increment the global module index. Useful mostly if you are manually
  /// manipulating or creating WaPC hosts via their internals.
  pub fn next_id() -> u64 {
    GLOBAL_MODULE_COUNT.fetch_add(1, Ordering::SeqCst)
  }

  /// Creates a new instance of a waPC-compliant host runtime paired with a given
  /// low-level engine provider
  pub fn new(
    engine: Box<dyn WebAssemblyEngineProvider>,
    sync_hostcall: Option<Arc<HostCallback>>,
    async_hostcall: Option<Arc<AsyncHostCallback>>,
  ) -> Result<Self> {
    let id = Self::next_id();
    let mh = WapcHost {
      engine: RefCell::new(engine),
      sync_hostcall,
      async_hostcall,
      id,
    };

    mh.initialize()?;

    Ok(mh)
  }

  fn initialize(&self) -> Result<()> {
    match self.engine.borrow_mut().init() {
      Ok(_) => Ok(()),
      Err(e) => Err(errors::Error::InitFailed(e.to_string())),
    }
  }

  /// Returns a reference to the unique identifier of this module. If a parent process
  /// has instantiated multiple `WapcHost`s, then the single static host callback function
  /// will contain this value to allow disambiguation of modules
  pub fn id(&self) -> u64 {
    self.id
  }

  /// Invokes the `__guest_call` function within the guest module as per the waPC specification.
  /// Provide an operation name and an opaque payload of bytes and the function returns a `Result`
  /// containing either an error or an opaque reply of bytes.
  ///
  /// It is worth noting that the _first_ time `call` is invoked, the WebAssembly module
  /// might incur a "cold start" penalty, depending on which underlying engine you're using. This
  /// might be due to lazy initialization or JIT-compilation.
  pub fn call(&self, op: &str, payload: &[u8]) -> Result<Vec<u8>> {
    let state = Arc::new(ModuleState::new(
      self.sync_hostcall.clone(),
      self.async_hostcall.clone(),
      self.id,
    ));
    let context = self
      .engine
      .borrow()
      .new_context(state.clone())
      .map_err(|e| crate::errors::Error::Context(e.to_string()))?;
    let mut context = WapcCallContext::new(context, state)?;
    context
      .call(op, payload)
      .map_err(|e| crate::errors::Error::GuestCallFailure(e.to_string()))
  }

  /// Invokes the `__guest_call` function within the guest module as per the waPC specification.
  /// Provide an operation name and an opaque payload of bytes and the function returns a `Result`
  /// containing either an error or an opaque reply of bytes.
  ///
  /// It is worth noting that the _first_ time `call` is invoked, the WebAssembly module
  /// might incur a "cold start" penalty, depending on which underlying engine you're using. This
  /// might be due to lazy initialization or JIT-compilation.
  pub fn call_async<T: AsRef<str>>(&self, op: T, payload: Vec<u8>) -> futures_core::future::BoxFuture<Result<Vec<u8>>> {
    let state = Arc::new(ModuleState::new(
      self.sync_hostcall.clone(),
      self.async_hostcall.clone(),
      self.id,
    ));
    let context = self.engine.borrow().new_context(state.clone());
    let op = op.as_ref().to_owned();

    Box::pin(async move {
      let mut context = WapcCallContext::new(
        context.map_err(|e| crate::errors::Error::Context(e.to_string()))?,
        state,
      )?;

      // Necessary to let context tasks spin up.
      tokio::task::yield_now().await;

      context
        .call_async(&op, payload)
        .await
        .map_err(|e| crate::errors::Error::GuestCallFailure(e.to_string()))
    })
  }

  /// Performs a live "hot swap" of the WebAssembly module. Since all internal waPC execution is assumed to be
  /// single-threaded and non-reentrant, this call is synchronous and so
  /// you should never attempt to invoke `call` from another thread while performing this hot swap.
  ///
  /// **Note**: if the underlying engine you've chosen is a JITting engine, then performing a swap
  /// will re-introduce a "cold start" delay upon the next function call.
  ///
  /// If you perform a hot swap of a WASI module, you cannot alter the parameters used to create the WASI module
  /// like the environment variables, mapped directories, pre-opened files, etc. Not abiding by this could lead
  /// to privilege escalation attacks or non-deterministic behavior after the swap.
  pub fn replace_module(&self, module: &[u8]) -> Result<()> {
    match self.engine.borrow_mut().replace(module) {
      Ok(_) => Ok(()),
      Err(e) => Err(errors::Error::ReplacementFailed(e.to_string())),
    }
  }
}

/// A builder for [WapcHost]s
#[must_use]
#[derive(Default)]
pub struct WapcHostBuilder {
  async_hostcall: Option<Arc<AsyncHostCallback>>,
  sync_hostcall: Option<Arc<HostCallback>>,
}

impl std::fmt::Debug for WapcHostBuilder {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("WapcHostBuilder")
      .field("async_hostcall", &self.async_hostcall.as_ref().map(|_| "Fn"))
      .field("sync_hostcall", &self.sync_hostcall.as_ref().map(|_| "Fn"))
      .finish()
  }
}

impl WapcHostBuilder {
  /// Instantiate a new [WapcHostBuilder].
  pub fn new() -> Self {
    Self::default()
  }

  /// Configure a synchronous callback for this WapcHost.
  pub fn callback(mut self, callback: Arc<HostCallback>) -> Self {
    self.sync_hostcall = Some(callback);
    self
  }

  /// Configure a synchronous callback for this WapcHost.
  pub fn async_callback(mut self, callback: Arc<AsyncHostCallback>) -> Self {
    self.async_hostcall = Some(callback);
    self
  }

  /// Configure a synchronous callback for this WapcHost.
  pub fn build(self, engine: Box<dyn WebAssemblyEngineProvider>) -> Result<WapcHost> {
    WapcHost::new(engine, self.sync_hostcall, self.async_hostcall)
  }
}

/// An isolated call context that is meant to be cheap to create and throw away.
pub struct WapcCallContext {
  context: Box<dyn ProviderCallContext + Send + Sync>,
  state: Arc<ModuleState>,
  id: u64,
}

impl std::fmt::Debug for WapcCallContext {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("WapcCallContext").field("state", &self.state).finish()
  }
}

impl WapcCallContext {
  fn new(mut context: Box<dyn ProviderCallContext + Send + Sync>, state: Arc<ModuleState>) -> Result<Self> {
    let id = GLOBAL_CONTEXT_COUNT.fetch_add(1, Ordering::SeqCst);
    context
      .init()
      .map_err(|e| crate::errors::Error::InitFailed(e.to_string()))?;

    Ok(Self { context, state, id })
  }

  /// Return the unique id associated with this context.
  #[must_use]
  pub fn id(&self) -> u64 {
    self.id
  }

  /// Invokes the `__guest_call` function within the guest module as per the waPC specification.
  /// Provide an operation name and an opaque payload of bytes and the function returns a `Result`
  /// containing either an error or an opaque reply of bytes.
  ///
  /// It is worth noting that the _first_ time `call` is invoked, the WebAssembly module
  /// might incur a "cold start" penalty, depending on which underlying engine you're using. This
  /// might be due to lazy initialization or JIT-compilation.
  pub fn call(&mut self, op: &str, payload: &[u8]) -> Result<Vec<u8>> {
    let inv = Invocation::new(op, payload.to_vec());
    let op_len = inv.operation.len();
    let msg_len = inv.msg.len();
    let (index, _rx) = self.state.new_invocation(inv);

    let callresult = match self.context.call(index, op_len as i32, msg_len as i32) {
      Ok(c) => c,
      Err(e) => {
        return Err(errors::Error::GuestCallFailure(e.to_string()));
      }
    };

    let state = self.state.take_state(index).unwrap();

    if callresult == 0 {
      // invocation failed
      match state.guest_error {
        Some(ref s) => Err(errors::Error::GuestCallFailure(s.clone())),
        None => Err(errors::Error::GuestCallFailure(
          "No error message set for call failure".to_owned(),
        )),
      }
    } else {
      // invocation succeeded
      match state.guest_response {
        Some(ref e) => Ok(e.clone()),
        None => match state.guest_error {
          Some(ref s) => Err(errors::Error::GuestCallFailure(s.clone())),
          None => Err(errors::Error::GuestCallFailure(
            "No error message OR response set for call success".to_owned(),
          )),
        },
      }
    }
  }

  /// Invokes the `__guest_call` function within the guest module as per the waPC specification.
  /// Provide an operation name and an opaque payload of bytes and the function returns a `Result`
  /// containing either an error or an opaque reply of bytes.
  ///
  /// It is worth noting that the _first_ time `call` is invoked, the WebAssembly module
  /// might incur a "cold start" penalty, depending on which underlying engine you're using. This
  /// might be due to lazy initialization or JIT-compilation.
  pub fn call_async(&mut self, op: &str, payload: Vec<u8>) -> futures_core::future::BoxFuture<Result<Vec<u8>>> {
    let inv = Invocation::new(op, payload);
    let msg_len = inv.msg.len();
    let op_len = inv.operation.len();
    let (index, rx) = self.state.new_invocation(inv);

    let result = self.context.call_async(index, op_len as i32, msg_len as i32);

    Box::pin(async move {
      let callresult = match result {
        Ok(c) => c,
        Err(e) => {
          return Err(errors::Error::GuestCallFailure(e.to_string()));
        }
      };

      if callresult > 0 {
        return Err(errors::Error::GuestCallFailure(
          "Call did not produce an error but returned an error status code.".to_owned(),
        ));
      }

      let status = rx
        .await
        .map_err(|_e| errors::Error::GuestCallFailure("Async wait failed".to_owned()))?;
      match status {
        CallStatus::Complete(state) => match state.guest_error {
          Some(ref s) => Err(errors::Error::GuestCallFailure(s.clone())),
          None => Err(errors::Error::GuestCallFailure(
            "No error message set for call failure".to_owned(),
          )),
        },
        CallStatus::Error(state) => match state.guest_response {
          Some(ref e) => Ok(e.clone()),
          None => match state.guest_error {
            Some(ref s) => Err(errors::Error::GuestCallFailure(s.clone())),
            None => Err(errors::Error::GuestCallFailure(
              "No error message OR response set for call success".to_owned(),
            )),
          },
        },
      }
    })
  }
}
