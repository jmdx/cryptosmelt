use jsonrpc_core::*;
use jsonrpc_core::futures::sink::Sink;
use jsonrpc_core::serde_json::{Map};
use jsonrpc_core::futures::sync::mpsc::*;
use jsonrpc_tcp_server::*;
use std::net::{SocketAddr, IpAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use lru_time_cache::*;
use schedule_recv::periodic_ms;
use std::thread;
use config::*;
use blocktemplate::*;
use unlocker::Unlocker;
use app::App;
use miner::Miner;
use regex::Regex;

#[derive(Default, Clone)]
struct Meta {
  peer_addr: Option<SocketAddr>,
  sender: Option<Sender<String>>,
}
impl Metadata for Meta {}

struct StratumServer {
  config: ServerConfig,
  app: Arc<App>,
  miner_connections: Mutex<LruCache<String, Arc<Miner>>>,
  miner_bans: Mutex<LruCache<IpAddr, bool>>,
  job_provider: Arc<JobProvider>,
  nonce_pattern: Regex,
}

impl StratumServer {
  fn new(app: Arc<App>, server_config: &ServerConfig, job_provider: Arc<JobProvider>)
         -> StratumServer {
    let time_to_live = Duration::from_secs(60 * 60 * 2);
    // We only issue short bans - these are just to keep people from being able to cheaply overload
    // the server by falsely submitting low-difficulty shares.
    let ban_length = Duration::from_secs(60 * 5);
    StratumServer {
      config: server_config.clone(),
      app,
      miner_connections: Mutex::new(
        LruCache::with_expiry_duration_and_capacity(time_to_live, server_config.max_connections.unwrap_or(10000))
      ),
      miner_bans: Mutex::new(
        LruCache::with_expiry_duration(ban_length),
      ),
      job_provider,
      nonce_pattern: Regex::new("[0-9a-f]{8}").unwrap()
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

  fn refresh_all_jobs(&self) {
    debug!("Refreshing {} jobs.", self.miner_connections.lock().unwrap().len());
    for (_, miner) in self.miner_connections.lock().unwrap().iter() {
      miner.retarget_job(&self.job_provider);
    }
  }

  fn login(&self, params: Map<String, Value>, meta: Meta) -> Result<Value> {
    if self.is_banned(&meta.peer_addr.unwrap().ip()) {
      return self.ban_message();
    }
    if let None = meta.peer_addr {
      return Err(Error::internal_error());
    }
    if let Some(&Value::String(ref login)) = params.get("login") {
      let mut login_parts = login.split(":");
      let address = login_parts.next().unwrap();
      let alias = login_parts.next().map(|a| a.to_owned());
      if alias.to_owned().map_or(false, |a| a.len() > 100) {
        return Err(Error::invalid_params("Miner alias can be at most 100 characters"));
      }
      if !self.app.address_pattern.is_match(address) {
        return Err(Error::invalid_params("Invalid wallet address in login parameters"));
      }
      let miner = Miner::new(address, alias, meta.peer_addr.unwrap(), meta.sender.unwrap().clone(),
                             self.config.starting_difficulty as usize);
      let response = json!({
        "id": &miner.id,
        "job": miner.get_job(&self.job_provider)?,
        "status": "OK",
      });
      self.miner_connections.lock().unwrap().insert(miner.id.to_owned(), Arc::new(miner));
      Ok(response)
    } else {
      Err(Error::invalid_params("Login address required"))
    }
  }

  fn getjob(&self, params: Map<String, Value>, _meta: Meta) -> Result<Value> {
    if let Some(miner) = self.getminer(&params) {
      miner.get_job(&self.job_provider)
    }
    else {
      Err(Error::invalid_params("No miner with this ID"))
    }
  }

  fn ban_ip(&self, ip: &IpAddr) {
    self.miner_bans.lock().unwrap().insert(ip.to_owned(), true);
  }

  fn is_banned(&self, ip: &IpAddr) -> bool {
    self.miner_bans.lock().unwrap().peek(ip)
      .unwrap_or(&false)
      .to_owned()
  }

  fn ban_message(&self) -> Result<Value> {
    Err(Error::invalid_params(
      "Your IP has received a short temporary ban due to an invalid share.  Usually this is \
       due to a mistake configuring xmr-stak/xmrig/cpuminer/etc.  Typically the relevant config \
       option is named something like 'currency' or 'hashtype' - that value in your config needs \
       to match up with the pool you are connecting to."
    ))
  }

  fn submit(&self, params: Map<String, Value>, meta: Meta) -> Result<Value> {
    if let Some(addr) = meta.peer_addr {
      if self.is_banned(&addr.ip()) {
        return self.ban_message();
      }

      if let Some(miner) = self.getminer(&params) {
        if !self.app.address_pattern.is_match(&miner.address) {
          return Err(Error::invalid_params("Miner ID must be alphanumeric"));
        }
        if let Some(&Value::String(ref job_id)) = params.get("job_id") {
          if let Some(job) = miner.jobs.lock().unwrap().get(job_id) {
            if let Some(&Value::String(ref nonce)) = params.get("nonce") {
              if !self.nonce_pattern.is_match(nonce) {
                return Err(Error::invalid_params("nonce must be 8 hex digits"));
              }
              miner.adjust_difficulty(job.difficulty, &self.config);

              return match job.check_submission(nonce) {
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
                JobResult::SharesRejected => {
                  info!("Banning IP {} due to bad share", addr.ip());
                  self.ban_ip(&addr.ip());
                  if let Err(err) = meta.sender.unwrap().close() {
                    info!("Failed to close connection while banning miner: {:?}", err);
                  }
                  Err(Error::invalid_params("Share rejected"))
                },
              }
            }
          }
        }
        debug!("Miner submitted incompatible parameters: {:?}", params)
      }
    }
    Err(Error::invalid_params("No miner with this ID"))
  }
}

/// The jsonrpc_macros crate would provide some nice macros, but is strict about protocol versions.
/// Some mining software doesn't send over the required protocol version field, but sends its
/// parameters in a map.  So we need to route permissively using add_method_with_meta, and parse
/// parameters as a map no matter what version the miner says it uses.
macro_rules! route_permissive {
  ( $route:expr, $handler:ident, $server:ident, $io:ident ) => {
    let handled_ref = $server.clone();
    $io.add_method_with_meta($route, move |params, meta: Meta| {
      match params {
        Params::Map(map) => handled_ref.$handler(map, meta),
        _ => Err(Error::invalid_params("Expected a params map")),
      }
    });
  }
}

pub fn init(app_ref: Arc<App>) {
  let unlocker = Unlocker::new(app_ref.clone());
  let job_provider = Arc::new(JobProvider::new(app_ref.clone()));
  let servers: Vec<Arc<StratumServer>> = app_ref.config.ports.iter().map(|server_config| {
    let mut io = MetaIoHandler::with_compatibility(Compatibility::Both);
    let pool_server: Arc<StratumServer> = Arc::new(
      StratumServer::new(app_ref.clone(), server_config, job_provider.clone())
    );
    route_permissive!("login", login, pool_server, io);
    route_permissive!("getjob", getjob, pool_server, io);
    route_permissive!("submit", submit, pool_server, io);

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
