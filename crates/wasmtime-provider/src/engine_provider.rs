use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;
use wapc::{GuestExports, ModuleState, ProviderCallContext, WasiParams, WebAssemblyEngineProvider};
use wasmtime::{AsContextMut, Engine, Instance, Linker, Module, Store, TypedFunc};

use super::Result;
use crate::store::{new_store, WapcStore};
use crate::wapc_wasmtime;

/// A waPC engine provider that encapsulates the Wasmtime WebAssembly runtime
#[allow(missing_debug_implementations)]
pub struct WasmtimeEngineProvider {
  module: Module,
  engine: Arc<Engine>,
  linker: Linker<WapcStore>,
  wasi_params: Option<WasiParams>,
}

impl Clone for WasmtimeEngineProvider {
  fn clone(&self) -> Self {
    let engine = self.engine.clone();

    let mut new = Self {
      module: self.module.clone(),
      wasi_params: self.wasi_params.clone(),
      engine,
      linker: self.linker.clone(),
    };
    new.init().unwrap();
    new
  }
}

impl WasmtimeEngineProvider {
  /// Creates a new instance of a [WasmtimeEngineProvider].
  pub fn new(buf: &[u8], wasi: Option<WasiParams>) -> Result<WasmtimeEngineProvider> {
    let engine = Engine::default();
    Self::new_with_engine(buf, engine, wasi)
  }

  #[cfg(feature = "cache")]
  /// Creates a new instance of a [WasmtimeEngineProvider] with caching enabled.
  pub fn new_with_cache(
    buf: &[u8],
    wasi: Option<WasiParams>,
    cache_path: Option<&std::path::Path>,
  ) -> Result<WasmtimeEngineProvider> {
    let mut config = wasmtime::Config::new();
    config.strategy(wasmtime::Strategy::Cranelift);
    if let Some(cache) = cache_path {
      config.cache_config_load(cache)?;
    } else if let Err(e) = config.cache_config_load_default() {
      warn!("Wasmtime cache configuration not found ({}). Repeated loads will speed up significantly with a cache configuration. See https://docs.wasmtime.dev/cli-cache.html for more information.",e);
    }
    config.wasm_reference_types(false);
    let engine = Engine::new(&config)?;
    Self::new_with_engine(buf, engine, wasi)
  }

  /// Creates a new instance of a [WasmtimeEngineProvider] from a separately created [wasmtime::Engine].
  pub fn new_with_engine(buf: &[u8], engine: Engine, wasi_params: Option<WasiParams>) -> Result<Self> {
    let module = Module::new(&engine, buf)?;

    let mut linker: Linker<WapcStore> = Linker::new(&engine);
    #[cfg(feature = "wasi")]
    wasmtime_wasi::add_to_linker(&mut linker, |s| s.wasi_ctx.as_mut().unwrap()).unwrap();

    Ok(WasmtimeEngineProvider {
      module,
      engine: Arc::new(engine),
      wasi_params,
      linker,
    })
  }
}

impl WebAssemblyEngineProvider for WasmtimeEngineProvider {
  fn new_context(
    &self,
    state: Arc<ModuleState>,
  ) -> std::result::Result<
    Box<(dyn ProviderCallContext + Send + Sync + 'static)>,
    Box<(dyn std::error::Error + Send + Sync + 'static)>,
  > {
    Ok(Box::new(WasmtimeCallContext::new(
      state,
      self.linker.clone(),
      &self.module,
      new_store(&self.wasi_params, &self.engine)?,
    )?))
  }

  fn replace(
    &mut self,
    _module: &[u8],
  ) -> std::result::Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
    unimplemented!();
  }
}

#[derive(Clone)]
pub(crate) enum AsyncChannel {
  Message(std::result::Result<(i32, i32), (i32, i32)>),
  Close,
}

struct WasmtimeCallContext {
  store: Arc<Mutex<Store<WapcStore>>>,
  instance: Instance,
  state: Arc<ModuleState>,
  task: JoinHandle<()>,
  sender: tokio::sync::mpsc::UnboundedSender<AsyncChannel>,
}

fn make_async_hostcall_handler(
  func: TypedFunc<(i32, i32), ()>,
  store: Arc<Mutex<Store<WapcStore>>>,
  mut rx: UnboundedReceiver<AsyncChannel>,
) -> JoinHandle<()> {
  tokio::spawn(async move {
    while let Some(result) = rx.recv().await {
      let (id, code) = match result {
        AsyncChannel::Message(Ok(v)) => v,
        AsyncChannel::Message(Err(v)) => v,
        AsyncChannel::Close => break,
      };
      trace!(id, "received async message");

      let mut store = store.lock();
      let _ = func.call(store.as_context_mut(), (id, code));
    }
  })
}

impl WasmtimeCallContext {
  pub(crate) fn new(
    state: Arc<ModuleState>,
    mut linker: Linker<WapcStore>,
    module: &Module,
    mut store: Store<WapcStore>,
  ) -> Result<Self> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    wapc_wasmtime::add_to_linker(&mut linker, &state, &tx)?;

    let instance = linker.instantiate(store.as_context_mut(), module)?;

    let func = instance
      .get_typed_func::<(i32, i32), (), _>(store.as_context_mut(), GuestExports::HostResponseReady.as_ref())
      .map_err(|_| crate::errors::Error::HostResponseReadyNotFound)?;
    let store = Arc::new(Mutex::new(store));

    let task = make_async_hostcall_handler(func, store.clone(), rx);

    Ok(Self {
      task,
      state,
      instance,
      store,
      sender: tx,
    })
  }
}

impl Drop for WasmtimeCallContext {
  fn drop(&mut self) {
    let _ = self.sender.send(AsyncChannel::Close);
    self.task.abort();
  }
}

impl ProviderCallContext for WasmtimeCallContext {
  fn call(
    &mut self,
    id: i32,
    op_length: i32,
    msg_length: i32,
  ) -> std::result::Result<i32, Box<(dyn std::error::Error + Send + Sync + 'static)>> {
    let mut store = self.store.lock();
    let func = self
      .instance
      .get_typed_func(store.as_context_mut(), GuestExports::GuestCall.as_ref())
      .map_err(|_| crate::errors::Error::GuestCallNotFound)?;

    let call = func.call(store.as_context_mut(), (id, op_length, msg_length));

    match call {
      Ok(result) => Ok(result),
      Err(e) => {
        error!("Failure invoking guest module handler: {:?}", e);
        self.state.set_guest_error(id, e.to_string());
        Ok(0)
      }
    }
  }

  fn call_async(
    &mut self,
    id: i32,
    op_length: i32,
    msg_length: i32,
  ) -> std::result::Result<i32, Box<(dyn std::error::Error + Send + Sync + 'static)>> {
    let mut store = self.store.lock();
    let func = self
      .instance
      .get_typed_func::<(i32, i32, i32), (), _>(store.as_context_mut(), GuestExports::AsyncGuestCall.as_ref())
      .map_err(|_| crate::errors::Error::AsyncGuestCallNotFound)?;

    let call = func.call(store.as_context_mut(), (id, op_length, msg_length));

    match call {
      Ok(_) => Ok(0),
      Err(e) => {
        error!("Failure invoking async guest module handler: {:?}", e);
        self.state.set_guest_error(id, e.to_string());
        self.state.set_guest_call_complete(id, 0, Vec::new());
        Ok(0)
      }
    }
  }

  fn init(&mut self) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for starter in wapc::wapc_functions::guest_exports::REQUIRED_STARTS.iter() {
      let mut store = self.store.lock();
      match self.instance.get_typed_func(store.as_context_mut(), starter) {
        Ok(func) => func
          .call(store.as_context_mut(), ())
          .map_err(|e| crate::errors::Error::InitializationFailed(e.into()))?,
        Err(_) => continue,
      };
    }
    Ok(())
  }
}
