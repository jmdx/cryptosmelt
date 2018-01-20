use longkeccak::keccak;
use std::sync::atomic::*;
use std::sync::*;
use std::result::Result as StdResult;
use std::cmp::min;
use uuid::*;
use jsonrpc_core::*;
use mithril::byte_string;
use concurrent_hashmap::*;
use app::App;
use cryptonote_utils::*;

pub struct SuccessfulBlock {
  pub id: String,
  pub blob: String,
}

pub enum JobResult {
  BlockFound(SuccessfulBlock),
  SharesAccepted,
  SharesRejected,
}

pub struct Job {
  pub id: String,
  pub hash_type: HashType,
  pub height: u64,
  pub difficulty: u64,
  pub diff_hex: String,
  pub hashing_blob: String,
  // TODO consider just keeping an Arc to the original template, storing this stuff is redundant
  pub template_blob: String,
  pub extra_nonce: String,
  pub reserved_offset: u32,
  pub network_difficulty: u64,
  submissions: ConcHashMap<String, bool>,
}

impl Job {
  pub fn submit(&self, nonce: &String) -> JobResult {
    if nonce.len() != 8 {
      // We expect a hex representing a 32 bit integer.  We don't care so much about validating that
      // it is purely hexadecimal chaaracters, though, since string_to_u8_array will just zero out
      // anything non-hexadecimal.
      // TODO actually, probably check that it's hex to be safe
      return JobResult::SharesRejected;
    }
    let previous_submission = self.submissions.insert(nonce.to_owned(), true);
    if let Some(_) = previous_submission {
      // TODO we'll probably want some auto banning functionality in place here
      return JobResult::SharesRejected;
    }
    // TODO check if the block is expired, may want to do away with the template reference and just
    // check against the current template, since anything of a lower height will be expired as long
    // as we only keep one template per height
    let blob = &self.hashing_blob;
    let (a, _) = blob.split_at(78);
    let (_, b) = blob.split_at(86);
    let hash_input = byte_string::string_to_u8_array(&format!("{}{}{}", a, nonce, b));
    let hash = cn_hash(&hash_input, &self.hash_type);
    let hash_val = byte_string::hex2_u64_le(&hash[48..]);
    let achieved_difficulty = u64::max_value() / hash_val;
    if achieved_difficulty >= self.difficulty {
      if achieved_difficulty >= self.network_difficulty {
        // The construction of the block ID is similar to the proof-of-work hash, except that:
        // - The hash input is prefixed with a length value before hashing.  It's not obvious why
        //   this is necessary, but probably has something to do with the fact that keccak is the
        //   first step in cryptonight, and that it wouldn't seem right to have the eventual block
        //   ID be there at some point in the cryptonight state.
        // - The fast hashing function, keccak, is used instead of cryptonight.
        let mut input_with_length = to_varint(hash_input.len());
        input_with_length.extend(&hash_input);
        // TODO use a helper function for this map/collect/join byte formatting
        let block_id: Vec<String> = keccak(&input_with_length)[..32].to_vec().iter()
          .map(|b| format!("{:02x}", b))
          .collect();;
        info!("Valid block candidate {}", block_id.join(""));
        let start_blob = &self.template_blob[..78];
        // TODO is there a good reason for "- 2"?
        let middle_blob = &self.template_blob[86..(self.reserved_offset as usize * 2 - 2)];
        let end_blob = &self.template_blob[(self.reserved_offset as usize * 2 + 16)..];
        let block_candidate = format!(
          "{}{}{}{}{}",
          start_blob,
          nonce,
          middle_blob,
          self.extra_nonce,
          end_blob
        );
        return JobResult::BlockFound(SuccessfulBlock {
          id: block_id.join(""),
          blob: block_candidate,
        });
      }
      return JobResult::SharesAccepted;
    } else {
      warn!("Bad job submission");
    }
    JobResult::SharesRejected
  }
}

pub struct JobProvider {
  template: RwLock<BlockTemplate>,
  nonce: AtomicUsize,
  app: Arc<App>,
  hash_type: HashType,
}

impl JobProvider {
  pub fn new(app: Arc<App>) -> JobProvider {
    let hash_type = match app.config.hash_type.to_lowercase().as_ref() {
      "cryptonight" => HashType::Cryptonight,
      "cryptonightlite" => HashType::CryptonightLite,
      _ => panic!("Invalid hash type in config.toml"),
    };
    JobProvider {
      template: RwLock::new(Default::default()),
      nonce: AtomicUsize::new(0),
      app,
      hash_type,
    }
  }

  pub fn get_job(&self, difficulty: u64) -> Option<Job> {
    // The network difficulty typically only exceeds the network difficulty shortly after firing
    // up a testnet.  Aside from that, sending out jobs higher than the network difficulty would
    // be unlikely, but undesirable, since it would mean telling miners not to send in completed
    // blocks.
    let job_id = &Uuid::new_v4().to_string();
    let template_data = self.template.read().unwrap();
    let capped_difficulty = min(difficulty, template_data.difficulty);
    let target_hex = get_target_hex(capped_difficulty);
    let new_nonce = self.nonce.fetch_add(1, Ordering::SeqCst);
    // TODO maybe hashing_blob_with_nonce should take in a u64
    let extra_nonce = &format!("{:016x}", new_nonce);
    let new_blob = template_data.hashing_blob_with_nonce(extra_nonce);
    // TODO do this more idiomatically
    match new_blob {
      Some(new_blob) => Some(Job {
        id: job_id.to_owned(),
        hash_type: self.hash_type.clone(),
        height: template_data.height,
        difficulty: capped_difficulty,
        diff_hex: target_hex,
        hashing_blob: new_blob,
        template_blob: template_data.blocktemplate_blob.to_owned(),
        extra_nonce: extra_nonce.to_owned(),
        reserved_offset: template_data.reserved_offset,
        network_difficulty: template_data.difficulty,
        submissions: Default::default(),
      }),
      None => None
    }
  }

  /// Refreshes the current template, returning true if there is a new one.
  pub fn fetch_new_template(&self) -> bool {
    let template = self.app.daemon.get_block_template();
    match template {
      Ok(template) => {
        if let Some(result) = template.get("result") {
          let parsed_template: StdResult<BlockTemplate, serde_json::Error> =
            serde_json::from_value(result.clone());
          if let Ok(new_template) = parsed_template {
            let mut current_template = self.template.write().unwrap();
            if new_template.prev_hash != current_template.prev_hash {
              info!("New block template of height {}.", new_template.height);
              *current_template = new_template;
              return true;
            }
          }
        }
      },
      Err(message) => warn!("Failed to get new block template: {:?}", message)
    }
    false
  }
}

#[derive(Deserialize, Default)]
pub struct BlockTemplate {
  // TODO eventually most of this stuff can be private
  pub blockhashing_blob: String,
  pub blocktemplate_blob: String,
  pub difficulty: u64,
  pub height: u64,
  pub prev_hash: String,
  pub reserved_offset: u32,
  pub status: String,
}

impl BlockTemplate {
  pub fn hashing_blob_with_nonce(&self, nonce: &str) -> Option<String> {
    // TODO do away with the [..] usage, turns out there are non-panic alternatives
    // TODO document this slicing stuff
    let miner_tx = format!(
      "{}{}",
      &self.blocktemplate_blob[86..((self.reserved_offset * 2 - 2) as usize)],
      nonce
    );
    let miner_tx_hash = keccak(&byte_string::string_to_u8_array(&miner_tx))[..32].to_vec();
    let hex_digits_left = (self.blocktemplate_blob.len() - miner_tx.len()) - 86;
    if (hex_digits_left - 2) % 64 != 0 {
      return None;
    }
    let mut tx_hashes = Vec::new();
    tx_hashes.push(miner_tx_hash);
    for tx_index in 0..(hex_digits_left / 64) {
      // TODO make these numbers less magic, maybe just increment an index for readability
      // TODO the "2" here assumes 1 byte for transaction count, that needs to be a varint as well
      let start = self.reserved_offset as usize * 2 + 16 + 64 * tx_index;
      tx_hashes.push(byte_string::string_to_u8_array(&self.blocktemplate_blob[start..(start + 64)]));
    }
    let num_hashes: Vec<String> = to_varint(tx_hashes.len()).iter()
      .map(|b| format!("{:02x}", b))
      .collect();
    let root_hash: Vec<String> = tree_hash(tx_hashes).iter()
      .map(|b| format!("{:02x}", b))
      .collect();
    return Some(
      format!("{}{}{}", &self.blockhashing_blob[..86], &root_hash.join(""), &num_hashes.join(""))
    );
  }
}

#[cfg(test)]
mod tests {
  use blocktemplate::*;

  #[test]
  fn test_parse_block_template() {
    let test_block = BlockTemplate {
      blockhashing_blob: "010094fed5d205e42c97122a7b61341c46881837099891d2b2587a0bde019cbae1688e41bc4\
    d70000000005c8e57bea6b5667f77529149756c249904fb346916f7580c18ea64ec793334e903".to_owned(),
      blocktemplate_blob: "010094fed5d205e42c97122a7b61341c46881837099891d2b2587a0bde019cbae1688e41bc\
    4d700000000001e1cf3701ffa5cf3705fbf3b1e40b02d2961caddbcd6294b41030ecf24fadc4229fc45c75df5def56d\
    c1841236db36380f8cce2840202bdba3913153bbbbd8c40a8b9409fe8944bb9964edd905506b558f8eadf027b858080\
    dd9da41702625f0a1c55924dedd94ae36929cfb99664176ff1d6417abfdc5bfb40daf20b9380a094a58d1d027151b66\
    783aa0ed7d3531dcc35b958945491922222327f9bd57693a18b252a6a80c0caf384a302022c8848debdf1f00e5f6a47\
    f0886e5caf027c8fd7e159277f1aa6c5a3796e49ca2b01bdcff031f0dd952991227c05512204eb76400cd8a06c30458\
    31783cd6fbdb9f50208000000000000000002cde625408d94764cf5244bff45ddb0f8d6d42d02b8c6afb99ae9dff33a\
    7bfcacae531ddf666352c45b25569c8d894ed8a327d9fb3c361ed0e7e0433190fe9fec".to_owned(),
      difficulty: 0,
      height: 0,
      prev_hash: "".to_owned(),
      reserved_offset: 285,
      status: "OK".to_owned(),
    };
    assert_eq!(test_block.blockhashing_blob,
               test_block.hashing_blob_with_nonce("0000000000000000").unwrap());

    // Kind of weird, but turns out it is possible to have blocks with just miner transactions.
    let test_empty_block = BlockTemplate {
      blockhashing_blob: "0100a5b6e1d205ae9d4d429436d01430aaed0fd1a3823c46a14b5c993e2085948e8bb148e862\
    b8000000007f8e1bb9aaccac84169ccf9a9a33ac704960e252e05218d19d93a147a396922901".to_owned(),
      blocktemplate_blob: "0100a5b6e1d205ae9d4d429436d01430aaed0fd1a3823c46a14b5c993e2085948e8bb148e8\
    62b80000000001bfd53701ff83d53705e7aee92d0236238d7c671cd670c1e5d145aa38407aa7c4caf78c9c3a5086126\
    c3d1e6d8bd48090dfc04a0288d398bf66e39e28888192a76534060cf698d293d3ed36ba25b23952ee8681a58080dd9d\
    a41702907aeacf368448e675dff25d15f74a2e55ca0155d09a6ee3ff22e9e8231e03e580a094a58d1d028cd86671141\
    36db4b05fffa7359039243594749b3241cce28a782d2ace58cb1180c0caf384a302020a1e50d39fa6615e3b3a6ca883\
    bd37a22f3870907bbc1dbbe70c1a6d6b4c1e342b01926d835f688b901dea5d5e2c0df2251a216d769b6cbabaa6fa81f\
    3797aba88cc0208000000000000000000".to_owned(),
      difficulty: 0,
      height: 0,
      prev_hash: "".to_owned(),
      reserved_offset: 283,
      status: "OK".to_owned(),
    };
    assert_eq!(test_empty_block.blockhashing_blob,
               test_empty_block.hashing_blob_with_nonce("0000000000000000").unwrap());
  }
}