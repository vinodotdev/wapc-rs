use wapc_guest::{register_dispatcher, BoxedFuture, HandlerResult};
mod custom_gen;

use custom_gen as generated;

#[no_mangle]
pub fn wapc_init() {
  println!("wapc_init");
  generated::Handlers::register_echo(echo);
  generated::Handlers::async_register_echo(async_echo);
  register_dispatcher(Box::new(generated::MyDispatcher {}))
}

fn echo(input: String) -> HandlerResult<String> {
  Ok(input)
}

fn async_echo(input: String) -> BoxedFuture<HandlerResult<String>> {
  Box::pin(async move {
    println!("in async echo");
    let result = wapc_guest::async_host_call("inner binding", "inner namespace", "inner name", b"heya").await?;
    println!("async host call result: {:?}", result);
    assert_eq!(result, vec![1, 2, 3]);
    Ok(input)
  })
}
