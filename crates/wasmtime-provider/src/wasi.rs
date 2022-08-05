use std::error::Error;
use std::ffi::OsStr;
use std::path::{Component, Path};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use wasi_common::WasiCtx;
use wasmtime_wasi::{ambient_authority, Dir};

pub(crate) fn init_ctx(
  preopen_dirs: &[(String, Dir)],
  argv: &[String],
  env: &[(String, String)],
) -> Result<WasiCtx, Box<dyn Error + Send + Sync>> {
  let mut ctx_builder = wasmtime_wasi::WasiCtxBuilder::new();

  ctx_builder = ctx_builder.inherit_stdio().args(argv)?.envs(env)?;

  for (name, file) in preopen_dirs {
    ctx_builder = ctx_builder.preopened_dir(file.try_clone()?, name)?;
  }

  Ok(ctx_builder.build())
}

use std::collections::HashMap;
static OPEN_DIRECTORIES: Lazy<Mutex<HashMap<String, Dir>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn get_dir(path: &str) -> Result<Dir, Box<dyn Error>> {
  let mut lock = OPEN_DIRECTORIES.lock();
  let dir = match lock.get(path) {
    Some(dir) => dir.try_clone()?,
    None => {
      let dir = Dir::open_ambient_dir(path, ambient_authority())?;
      lock.insert(path.to_owned(), dir.try_clone()?);
      dir
    }
  };
  Ok(dir)
}

pub(crate) fn compute_preopen_dirs(
  dirs: &[String],
  map_dirs: &[(String, String)],
) -> Result<Vec<(String, Dir)>, Box<dyn Error>> {
  let mut preopen_dirs = Vec::new();

  for path in dirs.iter() {
    let dir = get_dir(path)?;
    preopen_dirs.push((path.clone(), dir));
  }

  for (guest, host) in map_dirs.iter() {
    let dir = get_dir(host)?;
    preopen_dirs.push((guest.clone(), dir));
  }

  Ok(preopen_dirs)
}

#[allow(dead_code)]
pub(crate) fn compute_argv(module: &Path, module_args: &[String]) -> Vec<String> {
  // Add argv[0], which is the program name. Only include the base name of the
  // main wasm module, to avoid leaking path information.
  let mut result = vec![module
    .components()
    .next_back()
    .map(Component::as_os_str)
    .and_then(OsStr::to_str)
    .unwrap_or("")
    .to_owned()];

  // Add the remaining arguments.
  for arg in module_args.iter() {
    result.push(arg.clone());
  }

  result
}

#[cfg(feature = "wasi")]
pub(crate) fn init_wasi(params: &wapc::WasiParams) -> super::Result<WasiCtx> {
  trace!("initializing wasi");
  init_ctx(
    &compute_preopen_dirs(&params.preopened_dirs, &params.map_dirs)
      .map_err(|e| crate::errors::Error::PreopenedDirs(e.to_string()))?,
    &params.argv,
    &params.env_vars,
  )
  .map_err(|e| crate::errors::Error::InitializationFailed(e))
}
