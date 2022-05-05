use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicI32, Ordering};

use once_cell::sync::{Lazy, OnceCell};
use parking_lot::Mutex;
use wasm_rs_async_executor::single_threaded::tasks_count;

use crate::errors;

/// [CallResult] is the result for all waPC host and guest calls.
pub type CallResult = Result<Vec<u8>, Box<dyn std::error::Error + Sync + Send>>;

/// A generic type for the result of waPC operation handlers.
pub type HandlerResult<T> = Result<T, Box<dyn std::error::Error + Sync + Send>>;

/// Generic BoxedFuture type
pub type BoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

static CALL_NUM: AtomicI32 = AtomicI32::new(0);

/// The [__guest_call] function is required by waPC guests and should only be called by waPC hosts.
#[allow(unsafe_code, unreachable_pub)]
#[no_mangle]
pub extern "C" fn __guest_call(index: i32, op_len: i32, req_len: i32) -> i32 {
  println!(">> guest: __guest_call");
  let mut buf: Vec<u8> = Vec::with_capacity(req_len as _);
  let mut opbuf: Vec<u8> = Vec::with_capacity(op_len as _);

  unsafe {
    __guest_request(index, opbuf.as_mut_ptr(), buf.as_mut_ptr());
    // The two buffers have now been initialized
    buf.set_len(req_len as usize);
    opbuf.set_len(op_len as usize);
  };

  match REGISTRY.lock().get(&opbuf) {
    Some(handler) => match handler(&buf) {
      Ok(result) => {
        unsafe {
          __guest_response(index, result.as_ptr(), result.len());
        }
        1
      }
      Err(e) => {
        let errmsg = e.to_string();
        unsafe {
          __guest_error(index, errmsg.as_ptr(), errmsg.len());
        }
        0
      }
    },
    None => {
      let mut errmsg = b"No handler registered for function ".to_vec();
      errmsg.append(&mut opbuf);
      unsafe {
        __guest_error(index, errmsg.as_ptr(), errmsg.len());
      }
      0
    }
  }
}

/// The [__host_call_response_ready] function is required by waPC guests and should only be called by waPC hosts.
#[allow(unsafe_code, unreachable_pub, clippy::future_not_send)]
#[no_mangle]
pub extern "C" fn __host_call_response_ready(id: i32, code: i32) {
  println!(">> guest: __host_call_response_ready");
  let tx = ASYNC_HOST_CALLS.lock().remove(&id).unwrap();
  let _ = tx.send(code);
  exhaust_tasks();
}

/// The [__async_guest_call] function is required by waPC guests and should only be called by waPC hosts.
#[allow(unsafe_code, unreachable_pub, clippy::future_not_send)]
#[no_mangle]
pub extern "C" fn __async_guest_call(id: i32, op_len: i32, req_len: i32) {
  println!(">> guest: __async_guest_call");
  let mut buf: Vec<u8> = Vec::with_capacity(req_len as _);
  let mut opbuf: Vec<u8> = Vec::with_capacity(op_len as _);

  unsafe {
    __guest_request(id, opbuf.as_mut_ptr(), buf.as_mut_ptr());
    // The two buffers have now been initialized
    buf.set_len(req_len as usize);
    opbuf.set_len(op_len as usize);
  };
  let dispatcher = DISPATCHER.get().unwrap();

  println!(">> guest: spawning task");
  crate::executor::spawn(async move {
    println!(">> guest: in async task");
    let result = dispatcher
      .dispatch(Invocation {
        id,
        operation: String::from_utf8_lossy(&opbuf).to_string(),
        payload: buf,
      })
      .await;
    let code = match result {
      Ok(result) => {
        #[allow(unsafe_code)]
        unsafe {
          __guest_response(id, result.as_ptr(), result.len());
        }
        1
      }
      Err(e) => {
        let errmsg = e.to_string();
        #[allow(unsafe_code)]
        unsafe {
          __guest_error(id, errmsg.as_ptr(), errmsg.len());
        }
        0
      }
    };
    #[allow(unsafe_code)]
    unsafe {
      __guest_call_response_ready(id, code);
    }
  });
  exhaust_tasks();
}

#[derive(Debug)]
/// Invocation
pub struct Invocation {
  /// id of the call.
  pub id: i32,
  /// Operation name.
  pub operation: String,
  /// Payload
  pub payload: Vec<u8>,
}

static DISPATCHER: OnceCell<Box<dyn Dispatcher + Sync + Send>> = OnceCell::new();

/// Start the event loop
pub fn register_dispatcher(dispatcher: Box<dyn Dispatcher + Send + Sync>) {
  println!(">> guest: registering dispatcher");
  let _ = DISPATCHER.set(dispatcher);
}

/// Run all tasks until completion
pub fn exhaust_tasks() {
  crate::executor::run_while(Some(Box::new(move || {
    let num_in_flight = ASYNC_HOST_CALLS.lock().len();
    tasks_count() - num_in_flight > 0
  })));
}

/// Dispatcher trait for routing requests
pub trait Dispatcher {
  /// Dispatch method
  fn dispatch(&self, invocation: Invocation) -> BoxedFuture<CallResult>;
}

#[link(wasm_import_module = "wapc")]
extern "C" {
  /// The host's exported __console_log function.
  pub(crate) fn __console_log(ptr: *const u8, len: usize);

  /// The host's exported __host_call function.
  pub(crate) fn __host_call(
    call: i32,
    bd_ptr: *const u8,
    bd_len: usize,
    ns_ptr: *const u8,
    ns_len: usize,
    op_ptr: *const u8,
    op_len: usize,
    ptr: *const u8,
    len: usize,
  ) -> usize;

  /// The host's exported __host_call function.
  pub(crate) fn __async_host_call(
    call: i32,
    bd_ptr: *const u8,
    bd_len: usize,
    ns_ptr: *const u8,
    ns_len: usize,
    op_ptr: *const u8,
    op_len: usize,
    ptr: *const u8,
    len: usize,
  ) -> usize;

  /// The host's exported __host_response function.
  pub(crate) fn __host_response(call: i32, ptr: *mut u8);
  pub(crate) fn __host_response_len(call: i32) -> usize;

  pub(crate) fn __host_error(call: i32, ptr: *mut u8);
  pub(crate) fn __host_error_len(call: i32) -> usize;

  /// The host's exported __guest_response function.
  pub(crate) fn __guest_response(call: i32, ptr: *const u8, len: usize);
  /// The host's exported __guest_error function.
  pub(crate) fn __guest_error(call: i32, ptr: *const u8, len: usize);
  /// The host's exported __guest_request function.
  pub(crate) fn __guest_request(call: i32, op_ptr: *mut u8, ptr: *mut u8);

  pub(crate) fn __guest_call_response_ready(call: i32, code: i32);
}

type HandlerSignature = fn(&[u8]) -> CallResult;
type AsyncHandlerSignature = fn(Vec<u8>) -> BoxedFuture<CallResult>;

static REGISTRY: Lazy<Mutex<HashMap<Vec<u8>, HandlerSignature>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static ASYNC_REGISTRY: Lazy<Mutex<HashMap<Vec<u8>, AsyncHandlerSignature>>> = Lazy::new(|| Mutex::new(HashMap::new()));

static ASYNC_HOST_CALLS: Lazy<Mutex<HashMap<i32, tokio::sync::oneshot::Sender<i32>>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));

/// Register a handler for a waPC operation
pub fn register_function(name: &str, f: fn(&[u8]) -> CallResult) {
  REGISTRY.lock().insert(name.as_bytes().to_vec(), f);
}

/// Register a handler for a waPC operation
pub fn register_async_function(name: &str, f: AsyncHandlerSignature) {
  ASYNC_REGISTRY.lock().insert(name.as_bytes().to_vec(), f);
}

/// The function through which all host calls take place.
pub fn host_call(binding: &str, ns: &str, op: &str, msg: &[u8]) -> CallResult {
  let index = CALL_NUM.fetch_add(1, Ordering::SeqCst);

  #[allow(unsafe_code)]
  let callresult = unsafe {
    __host_call(
      index,
      binding.as_ptr(),
      binding.len(),
      ns.as_ptr(),
      ns.len(),
      op.as_ptr(),
      op.len(),
      msg.as_ptr(),
      msg.len(),
    )
  };
  if callresult != 1 {
    // call was not successful
    #[allow(unsafe_code)]
    let errlen = unsafe { __host_error_len(index) };

    let mut buf = Vec::with_capacity(errlen);
    let retptr = buf.as_mut_ptr();

    #[allow(unsafe_code)]
    unsafe {
      __host_error(index, retptr);
      buf.set_len(errlen);
    }

    Err(Box::new(errors::new(errors::ErrorKind::HostError(buf))))
  } else {
    // call succeeded
    #[allow(unsafe_code)]
    let len = unsafe { __host_response_len(index) };

    let mut buf = Vec::with_capacity(len);
    let retptr = buf.as_mut_ptr();

    #[allow(unsafe_code)]
    unsafe {
      __host_response(index, retptr);
      buf.set_len(len);
    }
    Ok(buf)
  }
}

#[allow(clippy::future_not_send)]
/// The function through which all host calls take place.
pub fn async_host_call<'a>(
  binding: &'a str,
  ns: &'a str,
  op: &'a str,
  msg: &'a [u8],
) -> BoxedFuture<Result<Vec<u8>, errors::Error>> {
  let index = CALL_NUM.fetch_add(1, Ordering::SeqCst);
  let (send, recv) = tokio::sync::oneshot::channel();
  let mut async_calls = ASYNC_HOST_CALLS.lock();
  async_calls.insert(index, send);

  #[allow(unsafe_code)]
  let callresult = unsafe {
    __async_host_call(
      index,
      binding.as_ptr(),
      binding.len(),
      ns.as_ptr(),
      ns.len(),
      op.as_ptr(),
      op.len(),
      msg.as_ptr(),
      msg.len(),
    )
  };
  println!(">> guest: wasm: async host call result: {}", callresult);

  Box::pin(async move {
    println!(">> guest: inner wasm task awaiting channel recv");
    if callresult != 0 {
      println!(">> guest: call failed");
      // call was not successful
      #[allow(unsafe_code)]
      let errlen = unsafe { __host_error_len(index) };

      let mut buf = Vec::with_capacity(errlen);
      let retptr = buf.as_mut_ptr();

      #[allow(unsafe_code)]
      unsafe {
        __host_error(index, retptr);
        buf.set_len(errlen);
      }
      Ok(buf)
    } else {
      // call succeeded
      match recv.await {
        Ok(code) => {
          println!(">> guest: call succeeded with code: {}", code);
          #[allow(unsafe_code)]
          let len = unsafe { __host_response_len(index) };

          let mut buf = Vec::with_capacity(len);
          let retptr = buf.as_mut_ptr();

          #[allow(unsafe_code)]
          unsafe {
            __host_response(index, retptr);
            buf.set_len(len);
          }
          Ok(buf)
        }
        Err(e) => {
          println!(">> guest: call failed : {}", e);
          Err(errors::new(errors::ErrorKind::Async(index, e.to_string())))
        }
      }
    }
  })
}

/// Log function that delegates to the host's __console_log function
#[cold]
#[inline(never)]
pub fn console_log(s: &str) {
  #[allow(unsafe_code)]
  unsafe {
    __console_log(s.as_ptr(), s.len());
  }
}
