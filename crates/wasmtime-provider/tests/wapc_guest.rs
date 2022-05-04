use std::sync::Arc;
use std::{fs::read, time::Duration};

use wapc::{errors, WapcHost};
use wapc_codec::messagepack::{deserialize, serialize};

#[test]
fn runs_wapc_guest() -> Result<(), errors::Error> {
  let buf = read("../../wasm/crates/wapc-guest-test/build/wapc_guest_test.wasm")?;

  let engine = wasmtime_provider::WasmtimeEngineProvider::new(&buf, None)?;
  let guest = WapcHost::new(
    Box::new(engine),
    Some(Arc::new(move |_a, _b, _c, _d, _e| Box::pin(async move { Ok(vec![]) }))),
  )?;

  let callresult = guest.call("echo", &serialize("hello world").unwrap())?;
  let result: String = deserialize(&callresult).unwrap();
  assert_eq!(result, "hello world");
  Ok(())
}

#[tokio::test]
async fn runs_async_wapc_guest() -> Result<(), errors::Error> {
  let buf = read("../../wasm/crates/wapc-guest-test/build/wapc_guest_test.wasm")?;

  let engine = wasmtime_provider::WasmtimeEngineProvider::new(&buf, None)?;
  let guest = WapcHost::new(
    Box::new(engine),
    Some(Arc::new(move |_a, _b, _c, _d, _e| {
      Box::pin(async move {
        let duration = 2000;
        println!("in host, sleeping {} ms.", duration);
        tokio::time::sleep(Duration::from_millis(duration)).await;
        println!("in host, done sleeping.");
        Ok(vec![1, 2, 3])
      })
    })),
  )?;
  let payload = serialize("hello world").unwrap();
  let callresult = guest.call_async("async_echo", payload).await?;
  let result: String = deserialize(&callresult).unwrap();
  assert_eq!(result, "hello world");
  Ok(())
}
