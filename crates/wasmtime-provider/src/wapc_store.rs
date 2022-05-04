use futures_core::future::BoxFuture;
use wapc::WasiParams;
use wasmtime::{Engine, Store};

use std::sync::Arc;

use crate::wasi::init_wasi;

#[cfg(not(feature = "wasi"))]
struct FakeWasiCtx {}
#[cfg(not(feature = "wasi"))]
type WasiCtx = FakeWasiCtx;
#[cfg(feature = "wasi")]
type WasiCtx = wasmtime_wasi::WasiCtx;

pub(crate) struct WapcStore {
  pub(crate) host_ready: Option<Arc<dyn Fn(i32, i32) -> BoxFuture<'static, ()> + Send + Sync>>,
  pub(crate) wasi_ctx: Option<WasiCtx>,
}

pub(crate) fn new_store(wasi_params: &Option<WasiParams>, engine: &Engine) -> super::Result<Store<WapcStore>> {
  let ctx = wasi_params.as_ref().and_then(|p| init_wasi(p).ok());
  Ok(Store::new(
    engine,
    WapcStore {
      wasi_ctx: ctx,
      host_ready: None,
    },
  ))
}
