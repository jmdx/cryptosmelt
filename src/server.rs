use jsonrpc_core::*;
use jsonrpc_core::serde_json::{Map};
use jsonrpc_tcp_server::*;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use concurrent_hashmap::*;
use uuid::*;
use reqwest;
use schedule_recv::periodic_ms;
use std::thread;

// TODO eventually this 'allow' will need to go away
#[allow(dead_code)]

struct Miner {
  miner_id: String,
  login: String,
  password: String,
  peer_addr: Option<SocketAddr>,
  difficulty: u64,
}

impl Miner {
  fn getjob(&self) -> String {
    "TODO write getjob, this will need to return a Map".to_owned()
  }
}

#[derive(Default, Clone)]
struct Meta {
  peer_addr: Option<SocketAddr>
}
impl Metadata for Meta {}

struct PoolServer {
  // TODO there will need to be expiry here
  miner_connections: ConcHashMap<String, Miner>
}

impl PoolServer {
  fn new()-> PoolServer {
    PoolServer {
      miner_connections: Default::default()
    }
  }

  fn getminer(&self, params: Map<String, Value>) -> Option<&Miner> {
    if let Some(&Value::String(ref id)) = params.get("id") {
      if let Some(miner) = self.miner_connections.find(id) {
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

  fn login(&self, params: Map<String, Value>, meta: Meta) -> Result<Value> {
    if let Some(&Value::String(ref login)) = params.get("login") {
      let mut response: Map<String, Value> = Map::new();
      let id = &Uuid::new_v4().to_string();
      // TODO add some validation on the login address
      response.insert("id".to_owned(), Value::String(id.to_owned()));
      let miner = Miner {
        miner_id: id.to_owned(),
        login: login.to_owned(),
        // TODO password isn't used, should probably go away
        password: "".to_owned(),
        peer_addr: meta.peer_addr,
        // TODO implement variable, configurable, fixed difficulties
        difficulty: 20000,
      };
      response.insert("job".to_owned(), Value::String(miner.getjob()));
      response.insert("status".to_owned(), Value::String("OK".to_owned()));
      self.miner_connections.insert(id.to_owned(), miner);
      Ok(Value::Object(response))
    } else {
      Err(Error::invalid_params("Login address required"))
    }
  }
}

// TODO this will probably go in another file
fn call_daemon(daemon_url: &str, method: &str, params: Value)
               -> reqwest::Result<Value> {
  let mut map = Map::new();
  map.insert("jsonrpc".to_owned(), Value::String("2.0".to_owned()));
  map.insert("id".to_owned(), Value::String("0".to_owned()));
  map.insert("method".to_owned(), Value::String(method.to_owned()));
  map.insert("params".to_owned(), params);

  let client = reqwest::Client::new();
  let mut res = client.post(daemon_url)
    .json(&map)
    .send()?;
  res.json()
}

// TODO probably take in a difficulty here
pub fn init(port: u16, daemon_url: String, pool_wallet: String) {
  // TODO take in 2 structs, ServerConfig and GlobalConfig
  let mut io = MetaIoHandler::default();
  //let mut pool_server: PoolServer = PoolServer::new();
  let pool_server: Arc<PoolServer> = Arc::new(PoolServer::new());
  let login_ref = pool_server.clone();
  let block_template: Arc<Mutex<Value>> = Arc::new(Mutex::new(Value::default()));
  io.add_method_with_meta("login", move |params, meta: Meta| {
    // TODO repeating this match isn't pretty
    match params {
      Params::Map(map) => login_ref.login(map, meta),
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
    .session_meta_extractor(|context: &RequestContext| {
      Meta {
        peer_addr: Some(context.peer_addr)
      }
    })
    .start(&SocketAddr::new("127.0.0.1".parse().unwrap(), port))
    .unwrap();
  // TODO make sure we refresh the template after every successful submit
  let template_refresh_ref = block_template.clone();
  thread::spawn(move || {
    // TODO maybe configurable block refresh interval
    let tick = periodic_ms(10000);
    loop {
      tick.recv().unwrap();
      // TODO make wallet_address configurable
      let params = json!({
        "wallet_address": pool_wallet,
        "reserve_size": 8
      });
      // TODO convert a lot of other stuff to the json! macro
      let template = call_daemon(&daemon_url, "getblocktemplate", params);
      match template {
        Ok(template) => *template_refresh_ref.lock().unwrap() = template,
        Err(message) => println!("Failed to get new block template: {}", message)
      }
      println!("New block template: {}", template_refresh_ref.lock().unwrap().to_string());
    }
  });
  server.wait();
}
