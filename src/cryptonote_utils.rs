use longkeccak::keccak;
use mithril::cryptonight::*;
use cryptonightlite;

#[derive(Clone)]
pub enum HashType {
  Cryptonight,
  CryptonightLite,
}

pub fn bytes_to_hex(bytes: Vec<u8>) -> String {
  let hexes: Vec<String> = bytes.iter()
    .map(|b| format!("{:02x}", b))
    .collect();
  hexes.join("")
}

pub fn cn_hash(input: &Vec<u8>, hash_type: &HashType) -> String {
  let aes = aes::new(aes::AESSupport::HW);
  match hash_type {
    &HashType::Cryptonight => hash::hash_alloc_scratchpad(input, &aes),
    &HashType::CryptonightLite => cryptonightlite::hash_alloc_scratchpad(input, &aes),
  }
}

/// Returns a representation of the miner's current difficulty, in a hex format which is sort of
/// a quirk of the stratum protocol.
pub fn get_target_hex(difficulty: u64) -> String {
  let difficulty_big_endian = format!("{:08x}", 0xffffffff / difficulty);
  format!(
    "{}{}{}{}",
    // This isn't a particularly elegant way of converting, but miners expect exactly 4
    // little-endian bytes so it's safe.
    &difficulty_big_endian[6..],
    &difficulty_big_endian[4..6],
    &difficulty_big_endian[2..4],
    &difficulty_big_endian[..2],
  )
}

/// From CNS-3 section 3
/// TODO document this, and really everything else, a bit better
#[allow(unused)] // We don't have any use for parsing varints right now, but might as well keep it
pub fn from_varint(source: &[u8]) -> (usize, usize) {
  if source[0] < 128 {
    return (source[0] as usize, 1);
  }
  let mut i = 0;
  let mut sum: usize = 0;
  while source[i] > 128 {
    let current_b128_digit = (source[i] - 128) as usize;
    // Shifting by i * 7 is multiplying by 128^i, since 128 is our base.
    sum += current_b128_digit << (i * 7);
    i += 1;
  }
  sum += (source[i] as usize) << (i * 7);
  return (sum, i + 1);
}

pub fn to_varint(number: usize) -> Vec<u8> {
  let mut remaining = number;
  let mut bytes = Vec::new();
  while remaining > 0 {
    bytes.push(((remaining % 128) + 128) as u8);
    remaining = remaining >> 7;
  }
  let marker_index = bytes.len() - 1;
  // The first value less than 128 marks the end of a varint byte sequence
  bytes[marker_index] -= 128;
  bytes
}



fn tree_hash_cnt(count: usize) -> usize {
  let mut i = 1;
  while i * 2 < count {
    i *= 2;
  }
  return i;
}

fn concat_and_hash(in1: &[u8], in2: &[u8]) -> Vec<u8> {
  let mut concatted_inputs = in1.to_vec();
  concatted_inputs.extend(in2.iter());
  return keccak(&concatted_inputs[..])[..32].to_vec();
}

// https://lab.getmonero.org/pubs/MRL-0002.pdf
pub fn tree_hash(hashes: Vec<Vec<u8>>) -> Vec<u8> {
  let count = hashes.len();
  if count == 1 {
    return hashes[0].to_vec();
  } else if count == 2 {
    return concat_and_hash(&hashes[0], &hashes[1]);
  } else {
    let mut cnt = tree_hash_cnt(count);
    let mut ints: Vec<Vec<u8>> = Vec::new();
    let slice_point = 2 * cnt - count;
    for i in 0..slice_point {
      ints.push(hashes[i].clone())
    }
    for _ in slice_point..count {
      ints.push(vec![0]);
    }
    let mut i = slice_point;
    for j in slice_point..cnt {
      ints[j] = concat_and_hash(&hashes[i], &hashes[i + 1]);
      i += 2;
    }

    while cnt > 2 {
      cnt /= 2;
      let mut ii = 0;
      for jj in 0..cnt {
        ints[jj] = concat_and_hash(&ints[ii], &ints[ii + 1]);
        ii += 2;
      }
    }
    return concat_and_hash(&ints[0], &ints[1]).to_vec();
  }
}


#[cfg(test)]
mod tests {
  use cryptonote_utils::*;

  #[test]
  fn test_bytes_to_hex () {
    assert_eq!(bytes_to_hex(vec![0xde, 0xad, 0xbe, 0xef, 0x13, 0x37]), "deadbeef1337")
  }

  #[test]
  fn test_hash() {
    use mithril::byte_string;
    let input = byte_string::string_to_u8_array("");
    assert_eq!(cn_hash(&input, &HashType::Cryptonight), "eb14e8a833fac6fe9a43b57b336789c46ffe93f2868452240720607b14387e11");
    // Test case taken from https://github.com/ExcitableAardvark/node-cryptonight-lite
    assert_eq!(cn_hash(&input, &HashType::CryptonightLite), "4cec4a947f670ffdd591f89cdb56ba066c31cd093d1d4d7ce15d33704c090611");
    let input2 = byte_string::string_to_u8_array("5468697320697320612074657374");
    assert_eq!(cn_hash(&input2, &HashType::CryptonightLite), "88e5e684db178c825e4ce3809ccc1cda79cc2adb4406bff93debeaf20a8bebd9");
  }


  #[test]
  fn test_varint() {
    assert_eq!(from_varint(&[42]), (42, 1));
    assert_eq!(from_varint(&[128 + 1, 42]), (42 * 128 + 1, 2));
    assert_eq!(from_varint(&[128 + 60, 128 + 61, 63]), (63 * 128 * 128 + 61 * 128 + 60, 3));

    assert_eq!(&to_varint(42)[..], &[42]);
    assert_eq!(&to_varint(42 * 128 + 1)[..], &[128 + 1, 42]);
    assert_eq!(&to_varint(63 * 128 * 128 + 61 * 128 + 60)[..], &[128 + 60, 128 + 61, 63]);
  }

  #[test]
  fn target_hex_correct() {
    assert_eq!(get_target_hex(5000), "711b0d00");
    assert_eq!(get_target_hex(20000), "dc460300");
    assert_eq!(get_target_hex(1), "ffffffff");
  }

  #[test]
  fn test_tree_hash() {
    use mithril::byte_string;
    // Test case pulled from the monero project's tests directory
    let concat_hash_tests = vec![
      byte_string::string_to_u8_array("21f750d5d938dd4ed1fa4daa4d260beb5b73509de9a9b145624d3f1afb671461"),
      byte_string::string_to_u8_array("b07d768cf1f5f8266b89ecdc150a2ad55ccd76d4c12d3a380b21862809a85af6"),
      byte_string::string_to_u8_array("23269a23ee1b4694b26aa317b5cd4f259925f6b3288a8f60fb871b1ad3ac00cb"),
      byte_string::string_to_u8_array("1e6c55eddfc438e1f3e7b638ea6026cc01495010bafdfd789c47dff282c1af4c"),
      byte_string::string_to_u8_array("6a8f83e5f2fca6940a756ef4faa15c7137082a7c31dffe0b2f5112d126ad4af1"),
      byte_string::string_to_u8_array("d536c0e626cc9d2fe1b72256f5285728558f22a3dbb36e0918bcfc01d4ae7284"),
      byte_string::string_to_u8_array("d0bfb8e90647cdb01c292a53a31ff3fe6f350882f1dae2b09374db45f4d54c67"),
      byte_string::string_to_u8_array("d3b4e0829c4f9f63ad235d8ef838d8fb39546d90d99bbd831aff55dbbb642e2b"),
      byte_string::string_to_u8_array("f529ceccd0479b9f194475c2a15143f0edac762e9bbce810436e765550c69e23"),
      byte_string::string_to_u8_array("4c22276c41d7d7e28c10afc5e144a9ce32aa9c0f28bb4fcf171af7d7404fa5e2"),
      byte_string::string_to_u8_array("8b79dc97bd4147f4df6d38b935bd83fb634414bae9d64a32ab45384fba5b8da5"),
      byte_string::string_to_u8_array("c147d51cd2a8f7f2a9c07b1bddc5b28b74bf0c0f0632ac2fc43d0d306dd1ac14"),
      byte_string::string_to_u8_array("81cabe60a358d6043d4733202d489664a929d6bf76a39828954846beb47a3baa"),
      byte_string::string_to_u8_array("cb35d2065cbe3ad34cf78bf895f6323a6d76fc1256306f58e4baecabd7a77938"),
      byte_string::string_to_u8_array("8c6bf2734897c193d39c343fce49a456f0ef84cf963593c5401a14621cc6ec1b"),
      byte_string::string_to_u8_array("ef01b53735ccb02bc96c5fd454105053e3b016174437ed83b25d2a79a88268f2"),
    ];
    let test_tree_hash = bytes_to_hex(tree_hash(concat_hash_tests));
    assert_eq!("2d0ad2566627b50cd45125e89e963433b212b368cd2d91662c44813ba9ec90c2", test_tree_hash);
  }
}