use std::collections::HashMap;
use std::fs::read;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::future::try_join_all;
use wapc::{errors, WapcHost, WasiParams};
use wapc_codec::messagepack::serialize;

#[test_log::test(tokio::test)]
async fn stress_test() -> Result<(), errors::Error> {
  let buf = read("../../../todoapp-be/build/todo.signed.wasm")?;

  let num_threads: u32 = 10;
  let num_calls: u32 = 50;

  let engine = wasmtime_provider::WasmtimeEngineProvider::new(
    &buf,
    Some(WasiParams::new(
      vec![],
      vec![("/".to_owned(), ".".to_owned())],
      vec![],
      vec![".".to_owned()],
    )),
  )?;
  let host = WapcHost::new(
    Box::new(engine),
    Some(Arc::new(|_a, _b, _c, _d, _e| Ok(b"".to_vec()))),
    Some(Arc::new(|_a, _b, _c, _d, _e| Box::pin(async move { Ok(b"".to_vec()) }))),
  )
  .unwrap();

  let now = Instant::now();
  let map: HashMap<String, String> = HashMap::new();
  let result = try_join_all((0..num_calls).map(|num| {
    println!("{}", num);
    let fut = host.call_async("list", serialize((1, map.clone(), map.clone())).unwrap());
    async move {
      println!("{}", num);
      fut.await
    }
  }))
  .await;
  let duration_all = now.elapsed();

  println!(
    "{} calls across {} threads took {}μs",
    num_calls,
    num_threads,
    duration_all.as_micros()
  );

  println!("{:?}", result);

  assert!(result.is_ok());
  let returns = result.unwrap();

  Ok(())
}
