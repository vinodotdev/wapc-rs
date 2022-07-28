use std::fs::read;
use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use wapc::{errors, WapcHost};
use wapc_codec::messagepack::{deserialize, serialize};

#[test_log::test(tokio::test)]
async fn runs_wapc_guest() -> Result<(), errors::Error> {
  let buf = read("../../wasm/crates/wapc-guest-test/build/wapc_guest_test.wasm")?;

  let engine = wasmtime_provider::WasmtimeEngineProvider::new(&buf, None)?;
  let guest = WapcHost::new(
    Box::new(engine),
    None,
    Some(Arc::new(move |_a, _b, _c, _d, _e| Box::pin(async move { Ok(vec![]) }))),
  )?;

  let callresult = guest.call("echo", &serialize("hello world").unwrap())?;
  let result: String = deserialize(&callresult).unwrap();
  assert_eq!(result, "hello world");
  Ok(())
}

#[test_log::test(tokio::test)]
async fn runs_async_wapc_guest() -> Result<(), errors::Error> {
  let buf = read("../../wasm/crates/wapc-guest-test/build/wapc_guest_test.wasm")?;

  let engine = wasmtime_provider::WasmtimeEngineProvider::new(&buf, None)?;
  let guest = WapcHost::new(
    Box::new(engine),
    None,
    Some(Arc::new(move |_a, _b, _c, _d, _e| {
      Box::pin(async move {
        let duration = 100;
        println!("in host, sleeping {} ms.", duration);
        tokio::time::sleep(Duration::from_millis(duration)).await;
        println!("in host, done sleeping.");
        Ok(vec![1, 2, 3])
      })
    })),
  )?;

  let results = join_all(vec![
    guest.call_async("async_echo", serialize("one").unwrap()),
    guest.call_async("async_echo", serialize("two").unwrap()),
    guest.call_async("async_echo", serialize("three").unwrap()),
    guest.call_async("async_echo", serialize("four").unwrap()),
    guest.call_async("async_echo", serialize("five").unwrap()),
  ])
  .await;
  let expected = vec![
    serialize("one").unwrap(),
    serialize("two").unwrap(),
    serialize("three").unwrap(),
    serialize("four").unwrap(),
    serialize("five").unwrap(),
  ];

  for (i, val) in results.into_iter().enumerate() {
    assert_eq!(val.unwrap(), expected[i]);
  }

  Ok(())
}
