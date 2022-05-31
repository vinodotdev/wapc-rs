use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::oneshot::Receiver;

use crate::{AsyncHostCallback, HostCallback, Invocation};

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

impl std::fmt::Debug for CallState {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("CallState")
      .field("guest_request", &self.guest_request)
      .field("guest_response", &self.guest_response)
      .field("host_response", &self.host_response)
      .field("guest_error", &self.guest_error)
      .field("host_error", &self.host_error)
      .finish()
  }
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
  pub(super) async_hostcall: Option<Arc<AsyncHostCallback>>,
  pub(super) sync_hostcall: Option<Arc<HostCallback>>,
  pub(super) call_index: AtomicI32,
  pub(super) id: u64,
}

impl std::fmt::Debug for ModuleState {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("ModuleState")
      .field("guest_calls", &self.calls)
      .field("call_index", &self.call_index)
      .field("id", &self.id)
      .finish()
  }
}

impl ModuleState {
  pub(crate) fn new(
    sync_hostcall: Option<Arc<HostCallback>>,
    async_hostcall: Option<Arc<AsyncHostCallback>>,
    id: u64,
  ) -> ModuleState {
    ModuleState {
      calls: Arc::new(Mutex::new(HashMap::new())),
      sync_hostcall,
      async_hostcall,
      call_index: AtomicI32::new(1),
      id,
    }
  }

  pub(crate) fn init_host_call(&self, id: i32) {
    let mut lock = self.calls.lock();
    trace!(module_id = self.id, id, "initializing host call state");
    lock.insert(id, CallState::default());
  }

  pub(crate) fn new_invocation(&self, inv: Invocation) -> (i32, Receiver<CallStatus>) {
    let id = self.call_index.fetch_add(1, Ordering::SeqCst);
    trace!(module_id = self.id, id, op = %inv.operation, "initializing guest call state");
    let mut lock = self.calls.lock();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let call_state = CallState {
      guest_request: Some(inv),
      channel: Some(tx),
      ..Default::default()
    };
    lock.insert(id, call_state);
    (id, rx)
  }

  /// Set an async invocation to done.
  pub fn finish_guest_invocation(&self, id: i32, code: i32) {
    trace!(module_id = self.id, id, code, "finishing guest invocation");
    if let Some(mut c) = self.calls.lock().remove(&id) {
      let tx = c.channel.take().unwrap();
      if code == 0 {
        let _ = tx.send(CallStatus::Complete(c));
      } else {
        let _ = tx.send(CallStatus::Error(c));
      }
    }
  }

  pub(crate) fn take_state(&self, id: i32) -> Option<CallState> {
    trace!(module_id = self.id, id, "dropping state");
    let mut lock = self.calls.lock();
    lock.remove(&id)
  }
}

impl ModuleState {
  /// Retrieves the value, if any, of the current guest request
  pub fn get_guest_request(&self, id: i32) -> Option<Invocation> {
    trace!(module_id = self.id, id, "get_guest_request");
    self.calls.lock().get(&id).and_then(|c| c.guest_request.clone())
  }

  /// Retrieves the value of the current host response
  pub fn get_host_response(&self, id: i32) -> Option<Vec<u8>> {
    trace!(module_id = self.id, id, "get_host_response");
    self.calls.lock().get(&id).and_then(|c| c.host_response.clone())
  }

  /// Sets a value indicating that an error occurred inside the execution of a guest call
  pub fn set_guest_error(&self, id: i32, error: String) {
    trace!(module_id = self.id, id, msg = %error, "set_guest_error");
    if let Some(mut c) = self.calls.lock().get_mut(&id) {
      c.guest_error = Some(error);
    }
  }

  /// Sets the value indicating the response data from a guest call
  pub fn set_guest_response(&self, id: i32, response: Vec<u8>) {
    trace!(module_id = self.id, id, "set_guest_response");
    if let Some(mut c) = self.calls.lock().get_mut(&id) {
      c.guest_response = Some(response);
    }
  }

  /// Sets the value indicating the response data from a guest call
  pub fn set_guest_call_complete(&self, id: i32, code: i32, response: Vec<u8>) {
    trace!(module_id = self.id, id, code, "set_guest_call_complete");
    if let Some(mut c) = self.calls.lock().get_mut(&id) {
      c.guest_response = Some(response);
    }
    self.finish_guest_invocation(id, code);
  }

  /// Queries the value of the current guest response
  pub fn get_guest_response(&self, id: i32) -> Option<Vec<u8>> {
    trace!(module_id = self.id, id, "get_guest_response");
    self.calls.lock().get(&id).and_then(|c| c.guest_response.clone())
  }

  /// Queries the value of the current host error
  pub fn get_host_error(&self, id: i32) -> Option<String> {
    trace!(module_id = self.id, id, "get_host_error");
    self.calls.lock().get(&id).and_then(|c| c.host_error.clone())
  }

  /// Invoked when the guest module wishes to make a call on the host
  pub fn do_host_call(
    &self,
    binding: String,
    namespace: String,
    operation: String,
    payload: Vec<u8>,
  ) -> Result<i32, Box<dyn std::error::Error>> {
    let id = self.call_index.fetch_add(1, Ordering::SeqCst);
    trace!(module_id = self.id, id, %binding, %namespace, %operation, "do_host_call");
    self.init_host_call(id);

    let host_callback = self.sync_hostcall.clone();
    let calls = self.calls.clone();
    let span = trace_span!("sync_task", module_id = self.id, id);
    let _guard = span.enter();
    trace!("starting");
    let result = {
      match host_callback {
        Some(ref f) => f(id, binding, namespace, operation, payload),
        None => Err("Missing host callback function!".into()),
      }
    };

    let code = match result {
      Ok(v) => {
        trace!("host call succeeded: {:?}", v);
        if let Some(mut call_state) = calls.lock().get_mut(&id) {
          call_state.host_response = Some(v);
        } else {
          error!(id, "call state not initialized");
        }
        id
      }
      Err(e) => {
        trace!("host call failed: {}", e);
        if let Some(call_state) = calls.lock().get_mut(&id) {
          call_state.host_error = Some(format!("{}", e));
        } else {
          error!(id, "call state not initialized");
        }
        0
      }
    };
    trace!(module_id = self.id, id, code, "executing callback");

    Ok(code)
  }

  /// Invoked when the guest module wishes to make a call on the host
  pub fn do_async_host_call(
    &self,
    binding: String,
    namespace: String,
    operation: String,
    payload: Vec<u8>,
    callback: Box<dyn FnOnce(i32, i32) + Send + Sync>,
  ) -> Result<i32, Box<dyn std::error::Error>> {
    let id = self.call_index.fetch_add(1, Ordering::SeqCst);
    trace!(module_id = self.id, id, %binding, %namespace, %operation, "do_async_host_call");
    self.init_host_call(id);

    let host_callback = self.async_hostcall.clone();
    let calls = self.calls.clone();
    let module_id = self.id;
    tokio::spawn(async move {
      let span = trace_span!("async_task", module_id, id);
      let _guard = span.enter();
      trace!("starting");
      let result = {
        match host_callback {
          Some(ref f) => f(id, binding, namespace, operation, payload).await,
          None => Err("Missing host callback function!".into()),
        }
      };

      let code = match result {
        Ok(v) => {
          trace!("async host call succeeded: {:?}", v);
          if let Some(mut call_state) = calls.lock().get_mut(&id) {
            call_state.host_response = Some(v);
          } else {
            error!(id, "call state not initialized");
          }
          id
        }
        Err(e) => {
          trace!("async host call failed: {}", e);
          if let Some(call_state) = calls.lock().get_mut(&id) {
            call_state.host_error = Some(format!("{}", e));
          } else {
            error!(id, "call state not initialized");
          }
          0
        }
      };
      trace!(module_id, id, code, "executing async callback");
      callback(id, code);
    });

    Ok(id)
  }

  /// Invoked when the guest module wants to write a message to the host's `stdout`
  pub fn do_console_log(&self, msg: &str) {
    trace!(id = self.id, msg, "guest console_log");
    println!("{}", msg);
  }
}
