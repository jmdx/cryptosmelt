use jsonrpc_core::*;
use jsonrpc_core::serde_json::{Map};
use jsonrpc_core::futures::sync::mpsc::*;
use jsonrpc_core::futures::*;
use jsonrpc_tcp_server::*;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::net::TcpStream;
use std::io::Write;
use std::time::*;
use std::sync::atomic::*;
use lru_time_cache::*;
use uuid::*;
use schedule_recv::periodic_ms;
use std::thread;
use config::*;
use blocktemplate::*;
use unlocker::Unlocker;
use influx_db_client::{Point, Points, Precision, Value as IxValue};
use app::App;

struct Miner {
  miner_id: String,
  login: String,
  password: String,
  peer_addr: SocketAddr,
  connection: Sender<String>,
  difficulty: AtomicUsize,
  jobs: Mutex<LruCache<String, Job>>,
  session_shares: AtomicUsize,
  session_start: SystemTime,
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
    if let Some(new_job) = job_provider.get_job(self.difficulty.load(Ordering::Relaxed) as u64) {
      // TODO maybe this method is superfluous now since it's mostly a passthrough
      // TODO probably cap the number of active jobs per miner
      let response = Ok(json!({
        "job_id": new_job.id,
        "blob": new_job.hashing_blob,
        "target": new_job.diff_hex,
      }));
      self.jobs.lock().unwrap().insert(new_job.id.to_owned(), new_job);
      return response;
    }
    Err(Error::internal_error())
  }

  fn adjust_difficulty(&self, job_provider: &Arc<JobProvider>, new_shares: u64, config: &ServerConfig) {
    let total_shares = self.session_shares.fetch_add(new_shares as usize, Ordering::SeqCst) as u64;
    let secs_since_start = SystemTime::now().duration_since(self.session_start)
      .expect("Session start is in the future, this shouldn't happen")
      .as_secs();
    let buffer_seconds = 60 * 5;
    let buffer_shares = config.starting_difficulty * buffer_seconds;
    let miner_hashrate = (total_shares + buffer_shares) / (secs_since_start + buffer_seconds);
    let ideal_difficulty = miner_hashrate * config.target_time;
    let actual_difficulty = self.difficulty.load(Ordering::Relaxed) as f64;
    let difficulty_ratio = (ideal_difficulty as f64) / actual_difficulty;
    if (difficulty_ratio - 1.0).abs() > 0.25 {
      debug!("Adjusting miner to difficulty {}, address {}", ideal_difficulty, self.login);
      // Each time we get a new block template, the miners need new jobs anyways - so we just leave
      // the retargeting to that process.  Calling retarget_job here would be slightly tricky since
      // we don't want to interrupt an in-progress RPC call from the miner.
      self.difficulty.store(ideal_difficulty as usize, Ordering::Relaxed);
    }
  }

  fn retarget_job(&self, job_provider: &Arc<JobProvider>) {
    let miner_job = self.get_job(job_provider);
    if let Ok(miner_job) = miner_job {
      let job_to_send = serde_json::to_string(&json!({
          "jsonrpc": 2.0,
          "method": "job",
          "params": miner_job,
        }));
      let connection = self.connection.clone();
      if let &Ok(ref job) = &job_to_send {
        connection.send(job.to_owned())
          .poll();
      }
      if let Err(err) = job_to_send {
        debug!("Failed to write job to {}: {:?}", &self.peer_addr, err);
      }
    }
  }
}

#[derive(Default, Clone)]
struct Meta {
  peer_addr: Option<SocketAddr>,
  sender: Option<Sender<String>>,
}
impl Metadata for Meta {}

struct PoolServer {
  config: ServerConfig,
  app: Arc<App>,
  // TODO there will need to be expiry here
  miner_connections: Mutex<LruCache<String, Arc<Miner>>>,
  job_provider: Arc<JobProvider>
}

impl PoolServer {
  fn new(app: Arc<App>, server_config: &ServerConfig, job_provider: Arc<JobProvider>)
         -> PoolServer {
    // TODO make sure that miners work with this, probably support the keepalive call
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
      // TODO add some validation on the login address
      if !self.app.address_pattern.is_match(login) {
        return Err(Error::invalid_params("Miner ID must be alphanumeric"));
      }
      let miner = Miner {
        miner_id: id.to_owned(),
        login: login.to_owned(),
        // TODO password isn't used, should probably go away
        password: "".to_owned(),
        peer_addr: meta.peer_addr.unwrap(),
        connection: meta.sender.unwrap().clone(),
        // TODO implement vardiff
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
        if let Some(job) = miner.jobs.lock().unwrap().get(job_id) {
          if let Some(&Value::String(ref nonce)) = params.get("nonce") {
            miner.adjust_difficulty(&self.job_provider, job.difficulty, &self.config);

            return match job.submit(nonce) {
              // TODO maybe insert the nonce and template, that would be cool because then the
              // server becomes fully auditable by connected miners
              // TODO split this up so it's not so deeply nested
              JobResult::BlockFound(block) => {
                // TODO move some of this over to a method on JobProvider, do something with the
                // result
                let _submission = self.app.daemon.submit_block(&block.blob);
                let mut share_log = Point::new("valid_share");
                share_log.add_tag("address", IxValue::String(miner.login.to_owned()));
                share_log.add_field("value", IxValue::Integer(job.difficulty as i64));
                let mut submission_log = Point::new("block_status");
                submission_log.add_tag("block", IxValue::String(block.id));
                submission_log.add_field("height", IxValue::Integer(job.height as i64));
                submission_log.add_field("status", IxValue::String("submitted".to_owned()));
                let mut to_insert = Points::new(share_log);
                to_insert.push(submission_log);
                let _ = self.app.db.write_points(to_insert, Some(Precision::Seconds), None).unwrap();
                Ok(Value::String("Submission accepted".to_owned()))
              },
              JobResult::SharesAccepted => {
                let mut share_log = Point::new("valid_share");
                share_log.add_tag("address", IxValue::String(miner.login.to_owned()));
                share_log.add_field("value", IxValue::Integer(job.difficulty as i64));
                let _ = self.app.db.write_point(share_log, Some(Precision::Seconds), None).unwrap();
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
