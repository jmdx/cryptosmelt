// Derived from Mithril's keccak.rs

//code taken from https://github.com/debris/tiny-keccak (and modified)

const PLEN: usize = 25;
const TLEN: usize = 144;

const RHO: [u32; 24] = [
  1,  3,  6, 10, 15, 21,
  28, 36, 45, 55,  2, 14,
  27, 41, 56,  8, 25, 43,
  62, 18, 39, 61, 20, 44
];

const PI: [usize; 24] = [
  10,  7, 11, 17, 18, 3,
  5, 16,  8, 21, 24, 4,
  15, 23, 19, 13, 12, 2,
  20, 14, 22,  9,  6, 1
];

const RC: [u64; 24] = [
  1u64, 0x8082u64, 0x800000000000808au64, 0x8000000080008000u64,
  0x808bu64, 0x80000001u64, 0x8000000080008081u64, 0x8000000000008009u64,
  0x8au64, 0x88u64, 0x80008009u64, 0x8000000au64,
  0x8000808bu64, 0x800000000000008bu64, 0x8000000000008089u64, 0x8000000000008003u64,
  0x8000000000008002u64, 0x8000000000000080u64, 0x800au64, 0x800000008000000au64,
  0x8000000080008081u64, 0x8000000000008080u64, 0x80000001u64, 0x8000000080008008u64
];

macro_rules! REPEAT4 {
    ($e: expr) => ( $e; $e; $e; $e; )
}

macro_rules! REPEAT5 {
    ($e: expr) => ( $e; $e; $e; $e; $e; )
}

macro_rules! REPEAT6 {
    ($e: expr) => ( $e; $e; $e; $e; $e; $e; )
}

macro_rules! REPEAT24 {
    ($e: expr, $s: expr) => (
        REPEAT6!({ $e; $s; });
        REPEAT6!({ $e; $s; });
        REPEAT6!({ $e; $s; });
        REPEAT5!({ $e; $s; });
        $e;
    )
}

macro_rules! FOR5 {
    ($v: expr, $s: expr, $e: expr) => {
        $v = 0;
        REPEAT4!({
            $e;
            $v += $s;
        });
        $e;
    }
}

/// keccak-f[1600]
pub fn keccakf(a: &mut [u64; PLEN]) {
  let mut b: [u64; 5] = [0; 5];
  let mut t: u64;
  let mut x: usize;
  let mut y: usize;

  for i in 0..24 {
    // Theta
    FOR5!(x, 1, {
            b[x] = 0;
            FOR5!(y, 5, {
                b[x] ^= a[x + y];
            });
        });

    FOR5!(x, 1, {
            FOR5!(y, 5, {
                a[y + x] ^= b[(x + 4) % 5] ^ b[(x + 1) % 5].rotate_left(1);
            });
        });

    // Rho and pi
    t = a[1];
    x = 0;
    REPEAT24!({
            b[0] = a[PI[x]];
            a[PI[x]] = t.rotate_left(RHO[x]);
        }, {
            t = b[0];
            x += 1;
        });

    // Chi
    FOR5!(y, 5, {
            FOR5!(x, 1, {
                b[x] = a[y + x];
            });
            FOR5!(x, 1, {
                a[y + x] = b[x] ^ ((!b[(x + 1) % 5]) & (b[(x + 2) % 5]));
            });
        });

    // Iota
    a[0] ^= RC[i];
  }
}

fn xorin(dst: &mut [u8], src: &[u8]) {
  for (d, i) in dst.iter_mut().zip(src) {
    *d ^= *i;
  }
}

fn transmute_u8(a: &mut [u64; PLEN]) -> &mut [u8; PLEN * 8] {
  unsafe { ::std::mem::transmute(a) }
}

fn transmute_u64(t: &mut [u8; TLEN]) -> &mut [u64; TLEN / 8] {
  unsafe { ::std::mem::transmute(t) }
}


/// The cryptonote-specific version of the keccak hashing function, taken from mithril and modified
/// a bit.  Mithril's version panics on long inputs, which is okay since mithril's use case only
/// has a fixed size requirement.  On the pool side, however, we need to hash whole transactions,
/// so need a slight generalization of the algorithm.
pub fn keccak(input: &[u8]) -> [u8; 200] {
  let mut a: [u64; PLEN] = [0; PLEN];
  let init_rate = 136; //200 - 512/4;
  let mut rate = init_rate;
  let inlen = input.len();
  let mut tmp: [u8; TLEN] = [0; TLEN];

  //first foldp
  let mut ip = 0;
  let mut l = inlen;
  while l >= rate {
    xorin(&mut transmute_u8(&mut a)[0..][..rate], &input[ip..]);
    keccakf(&mut a);
    ip += rate;
    l -= rate;
    rate = init_rate;
  }

  // This is the big point where we diverge from mithril's keccak.rs - theirs is equivalent on short
  // differs from cryptonote's implementation on long inputs.
  tmp[..l].copy_from_slice(&input[ip..]);
  //pad
  tmp[l] = 1;
  tmp[rate - 1] |= 0x80;

  let t64 = transmute_u64(&mut tmp);
  for i in 0..(rate/8) {
    a[i] ^= t64[i];
  }

  keccakf(&mut a);

  let t8 = transmute_u8(&mut a);
  return *t8;
}

#[test]
fn test_keccak() {
  use mithril::byte_string;

  let test_input = "fa22874bcc068879e8ef11a69f0722";
  assert_eq!(keccak(&byte_string::string_to_u8_array(test_input))[..32].to_vec(),
             byte_string::string_to_u8_array(
               "f20b3bcf743aa6fa084038520791c364cb6d3d1dd75841f8d7021cd98322bd8f").to_vec());
  let test_input_2 = "ea40e83cb18b3a242c1ecc6ccd0b7853a439dab2c569cfc6dc38a19f5c90acbf76aef9e\
  a3742ff3b54ef7d36eb7ce4ff1c9ab3bc119cff6be93c03e208783335c0ab8137be5b10cdc66ff3f89a1bddc6a1eed74f\
  504cbe7290690bb295a872b9e3fe2cee9e6c67c41db8efd7d863cf10f840fe618e7936da3dca5ca6df933f24f6954ba08\
  01a1294cd8d7e66dfafec";
  assert_eq!(keccak(&byte_string::string_to_u8_array(test_input_2))[..32].to_vec(),
             byte_string::string_to_u8_array(
               "344d129c228359463c40555d94213d015627e5871c04f106a0feef9361cdecb6").to_vec());
}