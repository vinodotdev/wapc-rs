use std::sync::Arc;
use std::time::Instant;

use futures_core::future::BoxFuture;
use wapc::WapcHost;
use wasmtime_provider::WasmtimeEngineProvider;

pub fn main() -> Result<(), wapc::errors::Error> {
  env_logger::init();
  let n = Instant::now();
  let file = &std::env::args()
    .nth(1)
    .expect("WASM file should be passed as the first CLI parameter");
  let func = &std::env::args()
    .nth(2)
    .expect("waPC guest function to call should be passed as the second CLI parameter");
  let payload = &std::env::args()
    .nth(3)
    .expect("The string payload to send should be passed as the third CLI parameter");

  let module_bytes = std::fs::read(file).expect("WASM could not be read");
  let engine = WasmtimeEngineProvider::new(&module_bytes, None)?;

  let host = WapcHost::new(Box::new(engine), None, Some(Arc::new(host_callback)))?;

  println!("Calling guest (wasm) function '{}'", func);
  let res = host.call(func, payload.to_owned().as_bytes())?;
  println!("Result - {}", ::std::str::from_utf8(&res).unwrap());
  println!("Elapsed - {}ms", n.elapsed().as_millis());
  Ok(())
}

fn host_callback(
  id: i32,
  bd: String,
  ns: String,
  op: String,
  payload: Vec<u8>,
) -> BoxFuture<'static, Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>> {
  println!(
    "Guest {} invoked '{}->{}:{}' on the host with a payload of '{}'",
    id,
    bd,
    ns,
    op,
    String::from_utf8(payload).unwrap()
  );
  Box::pin(async move { Ok(vec![]) })
}
