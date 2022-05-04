use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use futures_core::future::BoxFuture;
use parking_lot::Mutex;
use tokio::sync::oneshot::Receiver;

use crate::{HostCallback, Invocation};

#[allow(missing_debug_implementations)]
#[derive(Default)]
pub(crate) struct CallState {
  pub(super) guest_request: Option<Invocation>,
  pub(super) guest_response: Option<Vec<u8>>,
  pub(super) host_response: Option<Vec<u8>>,
  pub(super) guest_error: Option<String>,
  pub(super) host_error: Option<String>,
  pub(super) channel: Option<tokio::sync::oneshot::Sender<CallStatus>>,
}

pub(crate) enum CallStatus {
  Complete(CallState),
  Error(CallState),
}

#[allow(missing_debug_implementations)]
#[derive(Default)]
/// Module state is essentially a 'handle' that is passed to a runtime engine to allow it
/// to read and write relevant data as different low-level functions are executed during
/// a waPC conversation
pub struct ModuleState {
  pub(super) calls: Arc<Mutex<HashMap<i32, CallState>>>,
  pub(super) host_calls: Arc<Mutex<HashMap<i32, CallState>>>,
  pub(super) host_callback: Option<Arc<HostCallback>>,
  pub(super) call_index: AtomicI32,
  pub(super) id: u64,
}

impl ModuleState {
  pub(crate) fn new(host_callback: Option<Arc<HostCallback>>, id: u64) -> ModuleState {
    ModuleState {
      calls: Arc::new(Mutex::new(HashMap::new())),
      host_calls: Arc::new(Mutex::new(HashMap::new())),
      host_callback,
      call_index: AtomicI32::new(0),
      id,
    }
  }

  pub(crate) fn init_host_call(&self, id: i32) {
    let mut lock = self.host_calls.lock();
    println!("initialing call via init_call for id: {}", id);
    lock.insert(id, CallState::default());
  }

  pub(crate) fn new_invocation(&self, inv: Invocation) -> (i32, Receiver<CallStatus>) {
    let id = self.call_index.fetch_add(1, Ordering::SeqCst);
    let mut lock = self.calls.lock();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let call_state = CallState {
      guest_request: Some(inv),
      channel: Some(tx),
      ..Default::default()
    };
    println!("initialing call via new_invocation for id: {}", id);
    lock.insert(id, call_state);
    (id, rx)
  }

  /// Set an async invocation to done.
  pub fn finish_guest_invocation(&self, id: i32, code: i32) {
    println!("finishing guest invocation for call id: {}", id);
    if let Some(mut c) = self.calls.lock().remove(&id) {
      let tx = c.channel.take().unwrap();
      if code == 0 {
        let _ = tx.send(CallStatus::Complete(c));
      } else {
        let _ = tx.send(CallStatus::Error(c));
      }
    }
  }

  pub(crate) fn take_state(&self, index: i32) -> Option<CallState> {
    let mut lock = self.calls.lock();
    lock.remove(&index)
  }
}

impl ModuleState {
  /// Retrieves the value, if any, of the current guest request
  pub fn get_guest_request(&self, index: i32) -> Option<Invocation> {
    self.calls.lock().get(&index).and_then(|c| c.guest_request.clone())
  }

  /// Retrieves the value of the current host response
  pub fn get_host_response(&self, index: i32) -> Option<Vec<u8>> {
    self.calls.lock().get(&index).and_then(|c| c.host_response.clone())
  }

  /// Sets a value indicating that an error occurred inside the execution of a guest call
  pub fn set_guest_error(&self, index: i32, error: String) {
    if let Some(mut c) = self.calls.lock().get_mut(&index) {
      c.guest_error = Some(error);
    }
  }

  /// Sets the value indicating the response data from a guest call
  pub fn set_guest_response(&self, index: i32, response: Vec<u8>) {
    if let Some(mut c) = self.calls.lock().get_mut(&index) {
      c.guest_response = Some(response);
    }
  }

  /// Sets the value indicating the response data from a guest call
  pub fn set_guest_call_complete(&self, index: i32, code: i32, response: Vec<u8>) {
    if let Some(mut c) = self.calls.lock().get_mut(&index) {
      c.guest_response = Some(response);
    }
    self.finish_guest_invocation(index, code);
  }

  /// Queries the value of the current guest response
  pub fn get_guest_response(&self, index: i32) -> Option<Vec<u8>> {
    self.calls.lock().get(&index).and_then(|c| c.guest_response.clone())
  }

  /// Queries the value of the current host error
  pub fn get_host_error(&self, index: i32) -> Option<String> {
    self.calls.lock().get(&index).and_then(|c| c.host_error.clone())
  }

  /// Invoked when the guest module wishes to make a call on the host
  pub fn do_host_call(
    &self,
    index: i32,
    binding: String,
    namespace: String,
    operation: String,
    payload: Vec<u8>,
  ) -> Result<i32, Box<dyn std::error::Error>> {
    println!("performing host call");
    self.init_host_call(index);

    let cb = self.host_callback.clone();
    let calls = self.calls.clone();
    tokio::spawn(async move {
      let result = {
        match cb {
          Some(ref f) => f(index, binding, namespace, operation, payload).await,
          None => Err("Missing host callback function!".into()),
        }
      };
      match result {
        Ok(v) => {
          if let Some(mut call_state) = calls.lock().get_mut(&index) {
            call_state.host_response = Some(v);
          }
          1
        }
        Err(e) => {
          if let Some(call_state) = calls.lock().get_mut(&index) {
            call_state.host_error = Some(format!("{}", e));
          }
          0
        }
      }
    });

    Ok(0)
  }

  /// Invoked when the guest module wishes to make a call on the host
  pub fn do_async_host_call(
    &self,
    id: i32,
    binding: String,
    namespace: String,
    operation: String,
    payload: Vec<u8>,
    callback: Box<dyn FnOnce(i32, i32) -> BoxFuture<'static, ()> + Send + Sync>,
  ) -> Result<i32, Box<dyn std::error::Error>> {
    println!("performing async host call");
    self.init_host_call(id);

    let cb = self.host_callback.clone();
    let calls = self.calls.clone();
    tokio::spawn(async move {
      println!("in async host call task");
      let result = {
        match cb {
          Some(ref f) => f(id, binding, namespace, operation, payload).await,
          None => Err("Missing host callback function!".into()),
        }
      };

      let code = match result {
        Ok(v) => {
          if let Some(mut call_state) = calls.lock().get_mut(&id) {
            call_state.host_response = Some(v);
          }
          1
        }
        Err(e) => {
          if let Some(call_state) = calls.lock().get_mut(&id) {
            call_state.host_error = Some(format!("{}", e));
          }
          0
        }
      };
      callback(id, code);
    });

    Ok(0)
  }
  /// Invoked when the guest module wants to write a message to the host's `stdout`
  pub fn do_console_log(&self, msg: &str) {
    info!("Guest module {}: {}", self.id, msg);
  }
}
