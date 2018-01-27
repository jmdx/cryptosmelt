use app::App;
use std::sync::Arc;
use iron::prelude::*;
use iron::status;
use std::net::SocketAddr;
use std::thread;

pub fn init(app: Arc<App>) {
  fn hello_world(_: &mut Request) -> IronResult<Response> {
    Ok(Response::with((status::Ok, "Hello World!")))
  }
  let api_port = app.config.api_port;
  thread::spawn(move ||
    Iron::new(hello_world)
      .http(SocketAddr::new("0.0.0.0".parse().unwrap(), api_port))
      .unwrap()
  );
}
