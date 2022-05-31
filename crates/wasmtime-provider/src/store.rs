use wapc::WasiParams;
use wasmtime::{Engine, Store};

use crate::wasi::init_wasi;

#[cfg(not(feature = "wasi"))]
struct FakeWasiCtx {}
#[cfg(not(feature = "wasi"))]
type WasiCtx = FakeWasiCtx;
#[cfg(feature = "wasi")]
type WasiCtx = wasmtime_wasi::WasiCtx;

pub(crate) struct WapcStore {
  pub(crate) wasi_ctx: Option<WasiCtx>,
}

pub(crate) fn new_store(wasi_params: &Option<WasiParams>, engine: &Engine) -> super::Result<Store<WapcStore>> {
  trace!("creating new memory store");
  let params = wasi_params.clone().unwrap_or_default();
  let ctx = init_wasi(&params)?;
  Ok(Store::new(engine, WapcStore { wasi_ctx: Some(ctx) }))
}
