#[cfg(feature = "guest")]
use wapc_guest::prelude::*;

#[cfg(feature = "guest")]
pub struct Host {
  binding: String,
}

#[cfg(feature = "guest")]
impl Default for Host {
  fn default() -> Self {
    Host {
      binding: "default".to_string(),
    }
  }
}

/// Creates a named host binding
#[cfg(feature = "guest")]
pub fn host(binding: &str) -> Host {
  Host {
    binding: binding.to_string(),
  }
}

/// Creates the default host binding
#[cfg(feature = "guest")]
pub fn default() -> Host {
  Host::default()
}

#[cfg(feature = "guest")]
impl Host {
  pub fn echo(&self, input: String) -> HandlerResult<String> {
    host_call(
      &self.binding,
      "example:interface",
      "echo",
      &messagepack::serialize(input)?,
    )
    .map(|vec| {
      let resp = messagepack::deserialize::<String>(vec.as_ref()).unwrap();
      resp
    })
    .map_err(|e| e.into())
  }
}

#[cfg(feature = "guest")]
pub struct Handlers {}

#[cfg(feature = "guest")]
impl Handlers {
  pub fn register_echo(f: fn(String) -> HandlerResult<String>) {
    *ECHO.write().unwrap() = Some(f);
    register_function(&"echo", echo_wrapper);
  }
  pub fn async_register_echo(f: fn(String) -> BoxedFuture<HandlerResult<String>>) {
    *ASYNC_ECHO.write().unwrap() = Some(f);
    wapc_guest::register_async_function(&"echo", echo_async_wrapper);
  }
}

#[cfg(feature = "guest")]
static ECHO: once_cell::sync::Lazy<std::sync::RwLock<Option<fn(String) -> HandlerResult<String>>>> =
  once_cell::sync::Lazy::new(|| std::sync::RwLock::new(None));

#[cfg(feature = "guest")]
static ASYNC_ECHO: once_cell::sync::Lazy<std::sync::RwLock<Option<fn(String) -> BoxedFuture<HandlerResult<String>>>>> =
  once_cell::sync::Lazy::new(|| std::sync::RwLock::new(None));

#[cfg(feature = "guest")]
fn echo_wrapper(input_payload: &[u8]) -> CallResult {
  let input = messagepack::deserialize::<String>(input_payload)?;
  let lock = ECHO.read().unwrap().unwrap();
  let result = lock(input)?;
  Ok(messagepack::serialize(result)?)
}

#[cfg(feature = "guest")]
fn echo_async_wrapper(input_payload: Vec<u8>) -> BoxedFuture<CallResult> {
  Box::pin(async move {
    let input = messagepack::deserialize::<String>(&input_payload)?;
    let lock = ASYNC_ECHO.read().unwrap().unwrap();
    let result = lock(input).await?;
    Ok(messagepack::serialize(result)?)
  })
}

pub struct MyDispatcher {}
impl wapc_guest::Dispatcher for MyDispatcher {
  fn dispatch(&self, invocation: wapc_guest::Invocation) -> wapc_guest::BoxedFuture<wapc_guest::CallResult> {
    Box::pin(async move { dispatch(invocation).await })
  }
}

async fn dispatch(invocation: wapc_guest::Invocation) -> CallResult {
  println!("dispatch");
  match invocation.operation.as_str() {
    "async_echo" => echo_async_wrapper(invocation.payload).await,
    _ => panic!("operation {} not found", invocation.operation),
  }
}
