use std::sync::Arc;

use wapc::{IntoEnumIterator, ModuleState};
use wasmtime::{AsContext, Caller, FuncType, Linker, Memory, StoreContext, Trap, Val, ValType};

use crate::store::WapcStore;
fn get_caller_memory<T>(caller: &mut Caller<T>) -> Memory {
  let memory = caller.get_export("memory").map(|e| e.into_memory().unwrap());
  memory.unwrap()
}

fn get_vec_from_memory<'a, T: 'a>(store: impl Into<StoreContext<'a, T>>, mem: Memory, ptr: i32, len: i32) -> Vec<u8> {
  let data = mem.data(store);
  data[ptr as usize..(ptr + len) as usize].to_vec()
}

fn write_bytes_to_memory(store: impl AsContext, memory: Memory, ptr: i32, slice: &[u8]) {
  #[allow(unsafe_code)]
  unsafe {
    let raw = memory.data_ptr(store).offset(ptr as isize);
    raw.copy_from(slice.as_ptr(), slice.len());
  }
}

pub(crate) fn add_to_linker(
  linker: &mut Linker<WapcStore>,
  host: &Arc<ModuleState>,
  sender: &tokio::sync::mpsc::UnboundedSender<Result<(i32, i32), (i32, i32)>>,
) -> super::Result<()> {
  use wapc::HostExports;
  let module_name = "wapc";
  for export in HostExports::iter() {
    match export {
      HostExports::ConsoleLog => {
        let (extern_type, extern_fn) = linker_console_log(host.clone());
        linker.func_new(module_name, export.as_ref(), extern_type, extern_fn)?;
      }
      HostExports::HostCall => {
        let (extern_type, extern_fn) = linker_host_call(host.clone());
        linker.func_new(module_name, export.as_ref(), extern_type, extern_fn)?;
      }
      HostExports::AsyncHostCall => {
        let (extern_type, extern_fn) = linker_async_host_call(host.clone(), sender.clone());
        linker.func_new(module_name, HostExports::AsyncHostCall.as_ref(), extern_type, extern_fn)?;
      }
      HostExports::GuestRequest => {
        let (extern_type, extern_fn) = linker_guest_request(host.clone());
        linker.func_new(module_name, export.as_ref(), extern_type, extern_fn)?;
      }
      HostExports::HostResponse => {
        let (extern_type, extern_fn) = linker_host_response(host.clone());
        linker.func_new(module_name, export.as_ref(), extern_type, extern_fn)?;
      }
      HostExports::HostResponseLen => {
        let (extern_type, extern_fn) = linker_host_response_len(host.clone());
        linker.func_new(module_name, export.as_ref(), extern_type, extern_fn)?;
      }
      HostExports::GuestResponse => {
        let (extern_type, extern_fn) = linker_guest_response(host.clone());
        linker.func_new(module_name, export.as_ref(), extern_type, extern_fn)?;
      }
      HostExports::GuestError => {
        let (extern_type, extern_fn) = linker_guest_error(host.clone());
        linker.func_new(module_name, export.as_ref(), extern_type, extern_fn)?;
      }
      HostExports::HostError => {
        let (extern_type, extern_fn) = linker_host_error(host.clone());
        linker.func_new(module_name, export.as_ref(), extern_type, extern_fn)?;
      }
      HostExports::HostErrorLen => {
        let (extern_type, extern_fn) = linker_host_error_len(host.clone());
        linker.func_new(module_name, export.as_ref(), extern_type, extern_fn)?;
      }
      HostExports::GuestResponseReady => {
        let (extern_type, extern_fn) = linker_guest_response_ready(host.clone());
        linker.func_new(module_name, export.as_ref(), extern_type, extern_fn)?;
      }
    };
  }
  Ok(())
}

fn linker_guest_request(
  host: Arc<ModuleState>,
) -> (
  FuncType,
  impl Fn(Caller<'_, WapcStore>, &[Val], &mut [Val]) -> Result<(), Trap> + Send + Sync + 'static,
) {
  (
    FuncType::new(vec![ValType::I32, ValType::I32, ValType::I32], vec![]),
    move |mut caller, params, _results| {
      trace!(name = wapc::HostExports::GuestRequest.as_ref(), "calling import");
      let id = params[0].unwrap_i32();
      let op_ptr = params[1].unwrap_i32();
      let ptr = params[2].unwrap_i32();

      let invocation = host.get_guest_request(id);
      let memory = get_caller_memory(&mut caller);
      if let Some(inv) = invocation {
        trace!(op=%inv.operation, payload=?inv.msg, "guest call response");
        write_bytes_to_memory(caller.as_context(), memory, ptr, &inv.msg);
        write_bytes_to_memory(caller.as_context(), memory, op_ptr, inv.operation.as_bytes());
      } else {
        error!(id, "got guest_request for non-existent id");
      }
      Ok(())
    },
  )
}

fn linker_console_log(
  host: Arc<ModuleState>,
) -> (
  FuncType,
  impl Fn(Caller<'_, WapcStore>, &[Val], &mut [Val]) -> Result<(), Trap> + Send + Sync + 'static,
) {
  (
    FuncType::new(vec![ValType::I32, ValType::I32], vec![]),
    move |mut caller, params: &[Val], _results: &mut [Val]| {
      trace!(name = wapc::HostExports::ConsoleLog.as_ref(), "calling import");
      let ptr = params[0].unwrap_i32();
      let len = params[1].unwrap_i32();
      let memory = get_caller_memory(&mut caller);
      let vec = get_vec_from_memory(caller.as_context(), memory, ptr, len);

      let msg = std::str::from_utf8(&vec).unwrap();

      host.do_console_log(msg);
      Ok(())
    },
  )
}

fn linker_host_call(
  host: Arc<ModuleState>,
) -> (
  FuncType,
  impl Fn(Caller<'_, WapcStore>, &[Val], &mut [Val]) -> Result<(), Trap> + Send + Sync + 'static,
) {
  (
    FuncType::new(
      vec![
        ValType::I32,
        ValType::I32,
        ValType::I32,
        ValType::I32,
        ValType::I32,
        ValType::I32,
        ValType::I32,
        ValType::I32,
      ],
      vec![ValType::I32],
    ),
    move |mut caller, params: &[Val], results: &mut [Val]| {
      trace!(name = wapc::HostExports::HostCall.as_ref(), "calling import");
      let memory = get_caller_memory(&mut caller);

      let bd_ptr = params[0].unwrap_i32();
      let bd_len = params[1].unwrap_i32();
      let ns_ptr = params[2].unwrap_i32();
      let ns_len = params[3].unwrap_i32();
      let op_ptr = params[4].unwrap_i32();
      let op_len = params[5].unwrap_i32();
      let ptr = params[6].unwrap_i32();
      let len = params[7].unwrap_i32();

      let vec = get_vec_from_memory(caller.as_context(), memory, ptr, len);
      let bd_vec = get_vec_from_memory(caller.as_context(), memory, bd_ptr, bd_len);
      let bd = String::from_utf8(bd_vec).unwrap();
      let ns_vec = get_vec_from_memory(caller.as_context(), memory, ns_ptr, ns_len);
      let ns = String::from_utf8(ns_vec).unwrap();
      let op_vec = get_vec_from_memory(caller.as_context(), memory, op_ptr, op_len);
      let op = String::from_utf8(op_vec).unwrap();
      trace!(%op, "guest call invoking host operation");
      let call_id = host.do_host_call(bd, ns, op, vec);
      if let Ok(r) = call_id {
        results[0] = Val::I32(r);
      }
      Ok(())
    },
  )
}

fn linker_async_host_call(
  host: Arc<ModuleState>,
  sender: tokio::sync::mpsc::UnboundedSender<Result<(i32, i32), (i32, i32)>>,
) -> (
  FuncType,
  impl Fn(Caller<'_, WapcStore>, &[Val], &mut [Val]) -> Result<(), Trap> + Send + Sync + 'static,
) {
  (
    FuncType::new(
      vec![
        ValType::I32,
        ValType::I32,
        ValType::I32,
        ValType::I32,
        ValType::I32,
        ValType::I32,
        ValType::I32,
        ValType::I32,
      ],
      vec![ValType::I32],
    ),
    move |mut caller, params: &[Val], results: &mut [Val]| {
      trace!(name = wapc::HostExports::AsyncHostCall.as_ref(), "calling import");
      let memory = get_caller_memory(&mut caller);

      let bd_ptr = params[0].unwrap_i32();
      let bd_len = params[1].unwrap_i32();
      let ns_ptr = params[2].unwrap_i32();
      let ns_len = params[3].unwrap_i32();
      let op_ptr = params[4].unwrap_i32();
      let op_len = params[5].unwrap_i32();
      let ptr = params[6].unwrap_i32();
      let len = params[7].unwrap_i32();

      let vec = get_vec_from_memory(caller.as_context(), memory, ptr, len);
      let bd_vec = get_vec_from_memory(caller.as_context(), memory, bd_ptr, bd_len);
      let bd = String::from_utf8(bd_vec).unwrap();
      let ns_vec = get_vec_from_memory(caller.as_context(), memory, ns_ptr, ns_len);
      let ns = String::from_utf8(ns_vec).unwrap();
      let op_vec = get_vec_from_memory(caller.as_context(), memory, op_ptr, op_len);
      let op = String::from_utf8(op_vec).unwrap();
      trace!(%op, "guest call invoking async host operation");

      // let caller = caller.as_context_mut();
      let sender = sender.clone();

      let call_id = host.do_async_host_call(
        bd,
        ns,
        op,
        vec,
        Box::new(move |id: i32, code: i32| {
          trace!(id, code, "async host call complete");
          let _ = if code == 1 {
            sender.send(Ok((id, code)))
          } else {
            sender.send(Err((id, code)))
          };
        }),
      );
      if let Ok(r) = call_id {
        results[0] = Val::I32(r);
      }
      Ok(())
    },
  )
}

fn linker_host_response(
  host: Arc<ModuleState>,
) -> (
  FuncType,
  impl Fn(Caller<'_, WapcStore>, &[Val], &mut [Val]) -> Result<(), Trap> + Send + Sync + 'static,
) {
  (
    FuncType::new(vec![ValType::I32, ValType::I32], vec![]),
    move |mut caller, params: &[Val], _results: &mut [Val]| {
      trace!(name = wapc::HostExports::HostResponse.as_ref(), "calling import");
      let id = params[0].unwrap_i32();
      if let Some(ref e) = host.get_host_response(id) {
        let memory = get_caller_memory(&mut caller);
        let ptr = params[1].unwrap_i32();
        write_bytes_to_memory(caller.as_context(), memory, ptr, e);
      }
      Ok(())
    },
  )
}

fn linker_host_response_len(
  host: Arc<ModuleState>,
) -> (
  FuncType,
  impl Fn(Caller<'_, WapcStore>, &[Val], &mut [Val]) -> Result<(), Trap> + Send + Sync + 'static,
) {
  (
    FuncType::new(vec![ValType::I32], vec![ValType::I32]),
    move |mut _caller, params: &[Val], results: &mut [Val]| {
      trace!(name = wapc::HostExports::HostResponseLen.as_ref(), "calling import");
      let id = params[0].unwrap_i32();
      results[0] = Val::I32(match host.get_host_response(id) {
        Some(ref r) => r.len() as _,
        None => 0,
      });
      Ok(())
    },
  )
}

fn linker_guest_response_ready(
  host: Arc<ModuleState>,
) -> (
  FuncType,
  impl Fn(Caller<'_, WapcStore>, &[Val], &mut [Val]) -> Result<(), Trap> + Send + Sync + 'static,
) {
  (
    FuncType::new(vec![ValType::I32, ValType::I32], vec![]),
    move |mut _caller, params: &[Val], _results: &mut [Val]| {
      trace!(name = wapc::HostExports::GuestResponseReady.as_ref(), "calling import");
      let id = params[0].unwrap_i32();
      let code = params[1].unwrap_i32();
      host.finish_guest_invocation(id, code);
      Ok(())
    },
  )
}

fn linker_guest_response(
  host: Arc<ModuleState>,
) -> (
  FuncType,
  impl Fn(Caller<'_, WapcStore>, &[Val], &mut [Val]) -> Result<(), Trap> + Send + Sync + 'static,
) {
  (
    FuncType::new(vec![ValType::I32, ValType::I32, ValType::I32], vec![]),
    move |mut caller, params: &[Val], _results: &mut [Val]| {
      trace!(name = wapc::HostExports::GuestResponse.as_ref(), "calling import");
      let id = params[0].unwrap_i32();
      let ptr = params[1].unwrap_i32();
      let len = params[2].unwrap_i32();

      let memory = get_caller_memory(&mut caller);
      let vec = get_vec_from_memory(caller.as_context(), memory, ptr, len);
      host.set_guest_response(id, vec);
      Ok(())
    },
  )
}

fn linker_guest_error(
  host: Arc<ModuleState>,
) -> (
  FuncType,
  impl Fn(Caller<'_, WapcStore>, &[Val], &mut [Val]) -> Result<(), Trap> + Send + Sync + 'static,
) {
  (
    FuncType::new(vec![ValType::I32, ValType::I32, ValType::I32], vec![]),
    move |mut caller, params: &[Val], _results: &mut [Val]| {
      trace!(name = wapc::HostExports::GuestError.as_ref(), "calling import");
      let memory = get_caller_memory(&mut caller);
      let id = params[0].unwrap_i32();
      let ptr = params[1].unwrap_i32();
      let len = params[2].unwrap_i32();

      let vec = get_vec_from_memory(caller.as_context(), memory, ptr, len);
      host.set_guest_error(id, String::from_utf8(vec).unwrap());
      Ok(())
    },
  )
}

fn linker_host_error(
  host: Arc<ModuleState>,
) -> (
  FuncType,
  impl Fn(Caller<'_, WapcStore>, &[Val], &mut [Val]) -> Result<(), Trap> + Send + Sync + 'static,
) {
  (
    FuncType::new(vec![ValType::I32, ValType::I32], vec![]),
    move |mut caller, params: &[Val], _results: &mut [Val]| {
      trace!(name = wapc::HostExports::HostError.as_ref(), "calling import");
      let id = params[0].unwrap_i32();
      if let Some(ref e) = host.get_host_error(id) {
        let ptr = params[1].unwrap_i32();
        let memory = get_caller_memory(&mut caller);
        write_bytes_to_memory(caller.as_context(), memory, ptr, e.as_bytes());
      }
      Ok(())
    },
  )
}

fn linker_host_error_len(
  host: Arc<ModuleState>,
) -> (
  FuncType,
  impl Fn(Caller<'_, WapcStore>, &[Val], &mut [Val]) -> Result<(), Trap> + Send + Sync + 'static,
) {
  (
    FuncType::new(vec![ValType::I32], vec![ValType::I32]),
    move |mut _caller, params: &[Val], results: &mut [Val]| {
      trace!(name = wapc::HostExports::HostErrorLen.as_ref(), "calling import");
      let id = params[0].unwrap_i32();
      results[0] = Val::I32(match host.get_host_error(id) {
        Some(ref e) => e.len() as _,
        None => 0,
      });
      Ok(())
    },
  )
}
