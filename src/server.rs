use jsonrpc_core::*;
use jsonrpc_core::serde_json::{Map};
use jsonrpc_tcp_server::*;
use std::net::SocketAddr;
use std::sync::Arc;
use concurrent_hashmap::*;
use uuid::*;

// TODO eventually this 'allow' will need to go away
#[allow(dead_code)]

struct Miner {
  miner_id: String,
  login: String,
  password: String,
  ip: String,
  difficulty: u64,
}

impl Miner {
  fn getjob(&self) -> String {
    "TODO write getjob".to_owned()
  }
}

struct PoolServer {
  _miner_connections: ConcHashMap<String, Miner>
}

impl PoolServer {
  fn new()-> PoolServer {
    PoolServer {
      _miner_connections: Default::default()
    }
  }

  fn getminer(&self, params: Map<String, Value>) -> Option<&Miner> {
    if let Some(&Value::String(ref id)) = params.get("id") {
      if let Some(miner) = self._miner_connections.find(id) {
        let miner: &Miner = miner.get();
        Some(miner)
      } else {
        None
      }
    } else {
      None
    }
  }

  fn getjob(&self, params: Map<String, Value>) -> Result<Value> {
    if let Some(miner) = self.getminer(params) {
      Ok(Value::String(miner.getjob()))
    }
    else {
      Err(Error::invalid_params("No miner with this ID"))
    }
  }

  fn login(&self, params: Map<String, Value>) -> Result<Value> {
    if let Some(&Value::String(ref login)) = params.get("login") {
      let mut response: Map<String, Value> = Map::new();
      let id = &Uuid::new_v4().to_string();
      response.insert("id".to_owned(), Value::String(id.to_owned()));
      let miner = Miner {
        miner_id: id.to_owned(),
        login: login.to_owned(),
        // TODO password isn't used, should probably go away
        password: "".to_owned(),
        ip: "".to_owned(),
        // TODO implement variable, configurable, fixed difficulties
        difficulty: 20000,
      };
      self._miner_connections.insert(id.to_owned(), miner);
      Ok(Value::Object(response))
    }
    else {
      Err(Error::invalid_params("Login address required"))
    }
  }
}

// TODO probably take in a difficulty here
pub fn init(port: u16) {
  let mut io = IoHandler::new();
  //let mut pool_server: PoolServer = PoolServer::new();
  let pool_server: Arc<PoolServer> = Arc::new(PoolServer::new());
  let login_ref = pool_server.clone();
  io.add_method("login", move |params| {
    // TODO repeating this match isn't pretty
    match params {
      Params::Map(map) => login_ref.login(map),
      _ => Err(Error::invalid_params("Expected a params map")),
    }
  });

  let getjob_ref = pool_server.clone();
  io.add_method("getjob", move |params: Params| {
    // TODO repeating this match isn't pretty
    match params {
      Params::Map(map) => getjob_ref.getjob(map),
      _ => Err(Error::invalid_params("Expected a params map")),
    }
  });

  let _submit_ref = pool_server.clone();
  io.add_method("submit", |_params| {
    Ok(Value::String("hello".to_owned()))
  });

  let _keepalived_ref = pool_server.clone();
  io.add_method("keepalived", |_params| {
    Ok(Value::String("hello".to_owned()))
  });

  let server = ServerBuilder::new(io)
    .start(&SocketAddr::new("127.0.0.1".parse().unwrap(), port))
    .unwrap();

  server.wait();
}
