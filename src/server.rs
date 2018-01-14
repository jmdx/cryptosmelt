use jsonrpc_core::*;
use jsonrpc_core::serde_json::{Map};
use jsonrpc_tcp_server::*;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::result::Result as StdResult;
use concurrent_hashmap::*;
use uuid::*;
use reqwest;
use schedule_recv::periodic_ms;
use std::thread;
use config::*;
use data::InfluxClient;
use regex::Regex;
use blocktemplate::*;

// TODO eventually this 'allow' will need to go away
#[allow(dead_code)]
struct Miner {
  miner_id: String,
  login: String,
  password: String,
  peer_addr: SocketAddr,
  difficulty: u64,
  jobs: ConcHashMap<String, Job>,
}

impl Miner {
  fn get_job(&self, job_provider: &Arc<JobProvider>) -> Result<Value> {
    // Notes on the block template:
    // - reserve_size (8) is the amount of bytes to reserve so the pool can throw in an extra nonce
    // - the daemon returns result.reserved_offset, and that many bytes into
    //   result.blocktemplate_blob, we can write our 8 byte extra nonce
    // - the node pools use a global counter, but we might want the counter to be per-miner
    // - it might not even be necessary to use any counters
    //   (and just go with the first 8 bytes of the miner id)
    if let Some(new_job) = job_provider.get_job(self.difficulty) {
      // TODO maybe this method is superfluous now since it's mostly a passthrough
      // TODO probably cap the number of active jobs per miner
      let response = Ok(json!({
        "job_id": new_job.id,
        "blob": new_job.hashing_blob,
        "target": new_job.diff_hex,
      }));
      self.jobs.insert(new_job.id.to_owned(), new_job);
      return response;
    }
    Err(Error::internal_error())
  }
}

#[derive(Default, Clone)]
struct Meta {
  peer_addr: Option<SocketAddr>
}
impl Metadata for Meta {}

struct PoolServer {
  config: ServerConfig,
  // TODO there will need to be expiry here
  miner_connections: ConcHashMap<String, Miner>,
  job_provider: Arc<JobProvider>,
  db: Arc<InfluxClient>,
  address_pattern: Regex,
}

impl PoolServer {
  fn new(server_config: &ServerConfig, db: Arc<InfluxClient>, job_provider: Arc<JobProvider>)
         -> PoolServer {
    PoolServer {
      config: server_config.clone(),
      miner_connections: Default::default(),
      job_provider,
      db,
      address_pattern: Regex::new("[a-zA-Z0-9]+").unwrap(),
    }
  }

  fn getminer(&self, params: &Map<String, Value>) -> Option<&Miner> {
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
    if let Some(miner) = self.getminer(&params) {
      miner.get_job(&self.job_provider)
    }
    else {
      Err(Error::invalid_params("No miner with this ID"))
    }
  }

  fn login(&self, params: Map<String, Value>, meta: Meta) -> Result<Value> {
    if let None = meta.peer_addr {
      return Err(Error::internal_error());
    }
    if let Some(&Value::String(ref login)) = params.get("login") {
      let id = &Uuid::new_v4().to_string();
      // TODO add some validation on the login address
      if !self.address_pattern.is_match(login) {
        return Err(Error::invalid_params("Miner ID must be alphanumeric"));
      }
      let miner = Miner {
        miner_id: id.to_owned(),
        login: login.to_owned(),
        // TODO password isn't used, should probably go away
        password: "".to_owned(),
        peer_addr: meta.peer_addr.unwrap(),
        // TODO implement vardiff
        difficulty: self.config.difficulty,
        jobs: Default::default(),
      };
      let response = json!({
        "id": id,
        "job": miner.get_job(&self.job_provider)?,
        "status": "OK",
      });
      self.miner_connections.insert(id.to_owned(), miner);
      Ok(response)
    } else {
      Err(Error::invalid_params("Login address required"))
    }
  }

  fn submit(&self, params: Map<String, Value>) -> Result<Value> {
    if let Some(miner) = self.getminer(&params) {
      if !self.address_pattern.is_match(&miner.login) {
        return Err(Error::invalid_params("Miner ID must be alphanumeric"));
      }
      if let Some(&Value::String(ref job_id)) = params.get("job_id") {
        if let Some(job) = miner.jobs.find(job_id) {
          if let Some(&Value::String(ref nonce)) = params.get("nonce") {
            let job = job.get();
            return match job.submit(nonce) {
              // TODO maybe insert the nonce and template, that would be cool because then the
              // server becomes fully auditable by connected miners
              // TODO split this up so it's not so deeply nested
              JobResult::BlockFound(block) => {
                let params = json!([block]);
                // TODO move some of this over to a method on JobProvider, do something with the
                // result
                let _submission = call_daemon(&self.job_provider.daemon_url, "submitblock", params);
                let to_insert = format!("valid_share,address={} value={}", miner.login, job.difficulty);
                // TODO mining software seems reluctant to grab a job with the new template
                //self.job_provider.refresh();
                match self.db.write(&to_insert) {
                  Ok(_) => Ok(Value::String("Submission accepted".to_owned())),
                  Err(_) => Err(Error::internal_error())
                }
              },
              JobResult::SharesAccepted => {
                let to_insert = format!("valid_share,address={} value={}", miner.login, job.difficulty);
                match self.db.write(&to_insert) {
                  Ok(_) => Ok(Value::String("Submission accepted".to_owned())),
                  Err(_) => Err(Error::internal_error())
                }
              },
              JobResult::SharesRejected => Err(Error::invalid_params("Share rejected")),
            }
          }
        }
      }
    }
    Err(Error::invalid_params("No miner with this ID"))
  }
}

pub fn init(config: Config) {
  let config_ref = Arc::new(config);
  let influx_client = Arc::new(InfluxClient::new(config_ref.clone()));
  let inner_config_ref = config_ref.clone();
  let hash_type = match config_ref.hash_type.to_lowercase().as_ref() {
    "cryptonight" => HashType::Cryptonight,
    "cryptonightlite" => HashType::CryptonightLite,
    _ => panic!("Invalid hash type in config.toml"),
  };
  let job_provider = Arc::new(JobProvider::new(
    inner_config_ref.daemon_url.to_owned(),
    inner_config_ref.pool_wallet.to_owned(),
    hash_type,
  ));
  let servers: Vec<Arc<PoolServer>> = config_ref.ports.iter().map(|server_config| {
    let mut io = MetaIoHandler::with_compatibility(Compatibility::Both);
    let pool_server: Arc<PoolServer> = Arc::new(
      PoolServer::new(server_config, influx_client.clone(), job_provider.clone())
    );
    let login_ref = pool_server.clone();
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

    let submit_ref = pool_server.clone();
    io.add_method("submit", move |params| {
      // TODO repeating this match isn't pretty
      match params {
        Params::Map(map) => submit_ref.submit(map),
        _ => Err(Error::invalid_params("Expected a params map")),
      }
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
      .start(&SocketAddr::new("127.0.0.1".parse().unwrap(), server_config.port))
      .unwrap();
    thread::spawn(|| server.wait());
    pool_server
  }).collect();

  // TODO make sure we refresh the template after every successful submit
  let thread_config_ref = inner_config_ref.clone();
  let tick = periodic_ms(2000);
  loop {
    job_provider.refresh();
    tick.recv().unwrap();
  }
}
