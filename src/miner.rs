use std::sync::{Arc, Mutex};
use config::*;
use uuid::*;
use jsonrpc_core::*;
use jsonrpc_core::futures::sync::mpsc::*;
use jsonrpc_core::futures::*;
use std::net::SocketAddr;
use std::time::*;
use std::sync::atomic::*;
use lru_time_cache::*;
use blocktemplate::*;

pub struct Miner {
  pub id: String,
  pub login: String,
  pub password: String,
  pub peer_addr: SocketAddr,
  pub connection: Sender<String>,
  pub difficulty: AtomicUsize,
  pub jobs: Mutex<LruCache<String, Job>>,
  pub session_shares: AtomicUsize,
  pub session_start: SystemTime,
}

impl Miner {
  pub fn new(login: &str, peer_addr: SocketAddr, connection: Sender<String>, difficulty: usize) -> Miner {
    let id = &Uuid::new_v4().to_string();
    Miner {
      id: id.to_owned(),
      login: login.to_owned(),
      password: "".to_owned(),
      peer_addr,
      connection,
      difficulty: AtomicUsize::new(difficulty),
      jobs: Mutex::new(LruCache::with_capacity(3)),
      session_shares: AtomicUsize::new(0),
      session_start: SystemTime::now(),
    }
  }

  pub fn get_job(&self, job_provider: &Arc<JobProvider>) -> Result<Value> {
    // Notes on the block template:
    // - reserve_size (8) is the amount of bytes to reserve so the pool can throw in an extra nonce
    // - the daemon returns result.reserved_offset, and that many bytes into
    //   result.blocktemplate_blob, we can write our 8 byte extra nonce
    // - the node pools use a global counter, but we might want the counter to be per-miner
    // - it might not even be necessary to use any counters
    //   (and just go with the first 8 bytes of the miner id)
    if let Some(new_job) = job_provider.get_job(self.difficulty.load(Ordering::Relaxed) as u64) {
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

  pub fn adjust_difficulty(&self, new_shares: u64, config: &ServerConfig) {
    let total_shares = self.session_shares.fetch_add(new_shares as usize, Ordering::SeqCst) as u64;
    let secs_since_start = SystemTime::now().duration_since(self.session_start)
      .expect("Session start is in the future, this shouldn't happen")
      .as_secs();
    let buffer_seconds = 60 * 5;
    // These 'buffer' shares are not actually given to the miner, but just used as a smoothing
    // factor so that the miner's hashrate doesn't jump all over the place within the first few
    // minutes.  The number of buffer shares is equal to the number of shares a miner would get if
    // they connected to a stratum port, and mined at the exact hashrate that port is tuned for,
    // over a period of 5 minutes.
    let buffer_shares = (config.starting_difficulty * buffer_seconds) / config.target_time;
    // We then factor in those buffer shares to get a smoothed-out estimate of the miner's hashrate.
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

  pub fn retarget_job(&self, job_provider: &Arc<JobProvider>) {
    let miner_job = self.get_job(job_provider);
    if let Ok(miner_job) = miner_job {
      let job_to_send = serde_json::to_string(&json!({
          "jsonrpc": 2.0,
          "method": "job",
          "params": miner_job,
        }));
      let connection = self.connection.clone();
      if let &Ok(ref job) = &job_to_send {
        if let Err(err) = connection.send(job.to_owned()).poll() {
          warn!("Error polling sent job: {:?}", err)
        }
      }
      if let Err(err) = job_to_send {
        debug!("Failed to write job to {}: {:?}", &self.peer_addr, err);
      }
    }
  }
}
