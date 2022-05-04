use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use futures_core::future::BoxFuture;
use parking_lot::Mutex;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;
use wapc::{guest_exports, GuestExports, WapcCallContext};
use wapc::{ModuleState, WasiParams, WebAssemblyEngineProvider};
use wasmtime::{AsContextMut, Engine, Instance, Linker, Module, Store, StoreContext, StoreContextMut, TypedFunc};

use super::Result;
use crate::wapc_link;
use crate::wapc_store::{new_store, WapcStore};

struct EngineInner {
  host: Arc<ModuleState>,
}
/// A waPC engine provider that encapsulates the Wasmtime WebAssembly runtime
#[allow(missing_debug_implementations)]
pub struct WasmtimeEngineProvider {
  module: Module,
  inner: Option<Arc<EngineInner>>,
  engine: Arc<Engine>,
  linker: Linker<WapcStore>,
  wasi_params: Option<WasiParams>,
}

impl Clone for WasmtimeEngineProvider {
  fn clone(&self) -> Self {
    let engine = self.engine.clone();

    match &self.inner {
      Some(state) => {
        let mut new = Self {
          module: self.module.clone(),
          inner: None,
          wasi_params: self.wasi_params.clone(),
          engine,
          linker: self.linker.clone(),
        };
        new.init(state.host.clone()).unwrap();
        new
      }
      None => Self {
        module: self.module.clone(),
        inner: None,
        wasi_params: self.wasi_params.clone(),
        engine,
        linker: self.linker.clone(),
      },
    }
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
    config.strategy(wasmtime::Strategy::Cranelift)?;
    if let Some(cache) = cache_path {
      config.cache_config_load(cache)?;
    } else if let Err(e) = config.cache_config_load_default() {
      warn!("Wasmtime cache configuration not found ({}). Repeated loads will speed up significantly with a cache configuration. See https://docs.wasmtime.dev/cli-cache.html for more information.",e);
    }
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
      inner: None,
      engine: Arc::new(engine),
      wasi_params,
      linker,
    })
  }
}

impl WebAssemblyEngineProvider for WasmtimeEngineProvider {
  fn init(&mut self, host: Arc<ModuleState>) -> std::result::Result<(), Box<(dyn std::error::Error + Send + Sync)>> {
    wapc_link::add_to_linker(&mut self.linker, &host)?;

    self.inner = Some(Arc::new(EngineInner { host }));
    Ok(())
  }

  fn new_context(
    &self,
  ) -> std::result::Result<
    Box<(dyn WapcCallContext + Send + Sync + 'static)>,
    Box<(dyn std::error::Error + Send + Sync + 'static)>,
  > {
    let store = new_store(&self.wasi_params, &self.engine)?;

    Ok(Box::new(WasmtimeCallContext::new(
      self.inner.clone().unwrap(),
      self.linker.clone(),
      &self.module,
      store,
      &self.inner.as_ref().unwrap().host,
    )?))
  }

  fn replace(
    &mut self,
    _module: &[u8],
  ) -> std::result::Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
    Ok(())
    // info!(
    //   "HOT SWAP - Replacing existing WebAssembly module with new buffer, {} bytes",
    //   module.len()
    // );

    // let new_instance = instance_from_buffer(
    //   &mut self.store,
    //   &self.engine,
    //   module,
    //   &self.inner.as_ref().unwrap().host,
    //   &self.linker,
    // )?;
    // *self.inner.as_ref().unwrap().instance.write() = new_instance;

    // Ok(self.initialize()?)
  }
}

type HostCallMap = Arc<Mutex<HashMap<i32, tokio::sync::oneshot::Sender<std::result::Result<Vec<u8>, String>>>>>;

struct WasmtimeCallContext {
  store: Arc<Mutex<Store<WapcStore>>>,
  instance: Instance,
  wapc: Arc<EngineInner>,
  task: JoinHandle<()>,
  async_calls: HostCallMap,
}

fn make_async_handler(
  func: TypedFunc<(i32, i32), ()>,
  host: Arc<ModuleState>,
  store: Arc<Mutex<Store<WapcStore>>>,
  callmap: HostCallMap,
  mut rx: UnboundedReceiver<std::result::Result<(i32, i32), (i32, i32)>>,
) -> JoinHandle<()> {
  tokio::spawn(async move {
    while let Some(result) = rx.recv().await {
      let (id, code) = match result {
        Ok(v) => v,
        Err(v) => v,
      };
      let mut store = store.lock();
      let _ = func.call(store.as_context_mut(), (id, code));
      let sender = {
        let lock = callmap.lock();
        lock.remove(&id)
      };
      match sender {
        Some(tx) => match result {
          Ok((id, code)) => match host.get_host_response(id) {
            Some(bytes) => {
              tx.send(Ok(bytes));
            }
            None => {
              tx.send(Err(
                "Async host call completed but no data available to return".to_owned(),
              ));
            }
          },
          Err((id, code)) => match host.get_host_error(id) {
            Some(bytes) => {
              tx.send(Err(bytes));
            }
            None => {
              tx.send(Err(
                "Async host call completed with error but no error available to return".to_owned(),
              ));
            }
          },
        },
        None => todo!(),
      }
    }
  })
}

impl WasmtimeCallContext {
  pub(crate) fn new(
    wapc: Arc<EngineInner>,
    mut linker: Linker<WapcStore>,
    module: &Module,
    mut store: Store<WapcStore>,
    host: &Arc<ModuleState>,
  ) -> Result<Self> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    wapc_link::shadow_async_host_call(&mut linker, host, tx)?;
    let instance = linker.instantiate(store.as_context_mut(), module)?;

    let func = instance
      .get_typed_func::<(i32, i32), (), _>(store.as_context_mut(), GuestExports::HostResponseReady.as_ref())
      .map_err(|_| crate::errors::Error::HostResponseReadyNotFound)?;
    let store = Arc::new(Mutex::new(store));
    let inner_store = store.clone();
    let async_calls: HostCallMap = Arc::new(Mutex::new(HashMap::new()));

    let task = make_async_handler(func, host.clone(), store.clone(), async_calls.clone(), rx);

    let mut context = Self {
      task,
      wapc,
      instance,
      store,
      async_calls,
    };
    context.initialize()?;

    Ok(context)
  }

  fn initialize(&mut self) -> Result<()> {
    for starter in wapc::wapc_functions::guest_exports::REQUIRED_STARTS.iter() {
      let mut store = self.store.lock();
      let func = self
        .instance
        .get_typed_func(store.as_context_mut(), starter)
        .map_err(|e| crate::errors::Error::InitializationFailed(e.into()))?;
      func
        .call(store.as_context_mut(), ())
        .map_err(|e| crate::errors::Error::InitializationFailed(e.into()))?;
    }
    Ok(())
  }
}

impl WapcCallContext for WasmtimeCallContext {
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
        self.wapc.host.set_guest_error(id, e.to_string());
        Ok(0)
      }
    }
  }

  fn call_async(
    &mut self,
    id: i32,
    op_length: i32,
    msg_length: i32,
  ) -> BoxFuture<std::result::Result<Vec<u8>, Box<(dyn std::error::Error + Send + Sync + 'static)>>> {
    let mut lock = self.store.lock();

    let func = self
      .instance
      .get_typed_func::<(i32, i32, i32), (), _>(lock.as_context_mut(), GuestExports::AsyncGuestCall.as_ref())
      // .map_err(|_| crate::errors::Error::AsyncGuestCallNotFound)
      .unwrap();
    let call = func.call(lock.as_context_mut(), (id, op_length, msg_length));
    drop(lock);

    let wapc = self.wapc.clone();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    let mut lock = self.async_calls.lock();
    lock.insert(id, tx);

    let inner_store = self.store.clone();
    Box::pin(async move {
      match call {
        Ok(_) => match rx.await {
          Ok(v) => v,
          Err(e) => Err("Failure waiting for async call to complete".to_owned()),
        },
        Err(e) => {
          error!("Failure invoking guest module handler: {:?}", e);
          Err(e.to_string().into())
        }
      }
    })
  }
}

/*wtf did i do this for?


fn call_async(
  &mut self,
  id: i32,
  op_length: i32,
  msg_length: i32,
) -> BoxFuture<std::result::Result<Vec<u8>, Box<(dyn std::error::Error + Send + Sync + 'static)>>> {
  let mut lock = self.store.lock();

  let func = self
    .instance
    .get_typed_func::<(i32, i32, i32), (), _>(lock.as_context_mut(), GuestExports::AsyncGuestCall.as_ref())
    // .map_err(|_| crate::errors::Error::AsyncGuestCallNotFound)
    .unwrap();
  let call = func.call(lock.as_context_mut(), (id, op_length, msg_length));
  drop(lock);

  let wapc = self.wapc.clone();
  let (tx, mut rx) = tokio::sync::oneshot::channel();
  let mut lock = self.async_calls.lock();
  lock.insert(id, tx);

  let inner_store = self.store.clone();
  Box::pin(async move {
    match call {
      Ok(_) => match rx.await {
        Ok(v) => v,
        Err(e) => Err("Failure waiting for async call to complete".to_owned()),
      },
      Err(e) => {
        error!("Failure invoking guest module handler: {:?}", e);
        Err(e.to_string().into())
      }
    }
  })
}*/
