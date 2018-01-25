use jsonrpc_core::*;
use jsonrpc_core::serde_json::{Map};
use jsonrpc_core::futures::sync::mpsc::*;
use jsonrpc_tcp_server::*;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::*;
use std::sync::atomic::*;
use lru_time_cache::*;
use uuid::*;
use schedule_recv::periodic_ms;
use std::thread;
use config::*;
use blocktemplate::*;
use unlocker::Unlocker;
use app::App;
use miner::Miner;

#[derive(Default, Clone)]
struct Meta {
  peer_addr: Option<SocketAddr>,
  sender: Option<Sender<String>>,
}
impl Metadata for Meta {}

struct PoolServer {
  config: ServerConfig,
  app: Arc<App>,
  miner_connections: Mutex<LruCache<String, Arc<Miner>>>,
  job_provider: Arc<JobProvider>
}

impl PoolServer {
  fn new(app: Arc<App>, server_config: &ServerConfig, job_provider: Arc<JobProvider>)
         -> PoolServer {
    let time_to_live = ::std::time::Duration::from_secs(60 * 60 * 2);
    PoolServer {
      config: server_config.clone(),
      app,
      // TODO make max miners configurable
      miner_connections: Mutex::new(
        LruCache::with_expiry_duration_and_capacity(time_to_live, 10000)
      ),
      job_provider,
    }
  }

  fn getminer(&self, params: &Map<String, Value>) -> Option<Arc<Miner>> {
    if let Some(&Value::String(ref id)) = params.get("id") {
      self.miner_connections.lock().unwrap().get(id)
        .map(|miner| miner.clone())
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

  fn refresh_all_jobs(&self) {
    debug!("Refreshing {} jobs.", self.miner_connections.lock().unwrap().len());
    for (_, miner) in self.miner_connections.lock().unwrap().iter() {
      miner.retarget_job(&self.job_provider);
    }
  }

  fn login(&self, params: Map<String, Value>, meta: Meta) -> Result<Value> {
    if let None = meta.peer_addr {
      return Err(Error::internal_error());
    }
    if let Some(&Value::String(ref login)) = params.get("login") {
      let id = &Uuid::new_v4().to_string();
      if !self.app.address_pattern.is_match(login) {
        return Err(Error::invalid_params("Miner ID must be alphanumeric"));
      }
      // TODO use a constructor here
      let miner = Miner {
        miner_id: id.to_owned(),
        login: login.to_owned(),
        password: "".to_owned(),
        peer_addr: meta.peer_addr.unwrap(),
        connection: meta.sender.unwrap().clone(),
        difficulty: AtomicUsize::new(self.config.starting_difficulty as usize),
        jobs: Mutex::new(LruCache::with_capacity(3)),
        session_shares: AtomicUsize::new(0),
        session_start: SystemTime::now(),
      };
      let response = json!({
        "id": id,
        "job": miner.get_job(&self.job_provider)?,
        "status": "OK",
      });
      self.miner_connections.lock().unwrap().insert(id.to_owned(), Arc::new(miner));
      Ok(response)
    } else {
      Err(Error::invalid_params("Login address required"))
    }
  }

  fn submit(&self, params: Map<String, Value>) -> Result<Value> {
    if let Some(miner) = self.getminer(&params) {
      if !self.app.address_pattern.is_match(&miner.login) {
        return Err(Error::invalid_params("Miner ID must be alphanumeric"));
      }
      if let Some(&Value::String(ref job_id)) = params.get("job_id") {
        // TODO probably make a method on Miner
        if let Some(job) = miner.jobs.lock().unwrap().get(job_id) {
          if let Some(&Value::String(ref nonce)) = params.get("nonce") {
            miner.adjust_difficulty(job.difficulty, &self.config);

            return match job.submit(nonce) {
              JobResult::BlockFound(block) => {
                match self.app.daemon.submit_block(&block.blob) {
                  Ok(_) => self.app.db.block_found(block, &miner, &job),
                  Err(err) => warn!("Failed to send block to daemon: {:?}", err)
                };
                Ok(Value::String("Submission accepted".to_owned()))
              },
              JobResult::SharesAccepted => {
                self.app.db.shares_accepted(&miner, &job);
                Ok(Value::String("Submission accepted".to_owned()))
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
  // TODO maybe shuffle this stuff into App
  let app_ref = Arc::new(App::new(config));
  let unlocker = Unlocker::new(app_ref.clone());
  let job_provider = Arc::new(JobProvider::new(app_ref.clone()));
  let servers: Vec<Arc<PoolServer>> = app_ref.config.ports.iter().map(|server_config| {
    let mut io = MetaIoHandler::with_compatibility(Compatibility::Both);
    let pool_server: Arc<PoolServer> = Arc::new(
      PoolServer::new(app_ref.clone(), server_config, job_provider.clone())
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
          peer_addr: Some(context.peer_addr),
          sender: Some(context.sender.clone()),
        }
      })
      .start(&SocketAddr::new("0.0.0.0".parse().unwrap(), server_config.port))
      .unwrap();
    thread::spawn(|| server.wait());
    pool_server
  }).collect();

  let tick = periodic_ms(2000);
  let mut ticks_since_refresh = 0;
  loop {
    if job_provider.fetch_new_template() || ticks_since_refresh > 10 {
      debug!("Refreshing jobs on {} servers", servers.len());
      for server in servers.iter() {
        server.refresh_all_jobs();
      }
      ticks_since_refresh = 0;
    }
    unlocker.refresh();
    tick.recv().unwrap();
    ticks_since_refresh += 1;
  }
}
