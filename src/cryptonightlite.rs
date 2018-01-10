// Derived from Mithril's src/cryptonight/hash.rs

use mithril::cryptonight::aes::{AES};
use mithril::u64x2::u64x2;
use std::boxed::Box;

use groestl::{Digest, Groestl256};
use mithril::byte_string;

use blake;
use jhffi;
use skeinffi;
use std::cmp::min;

pub const MEM_SIZE : usize = 2097152 / 32;
const ITERATIONS : u32 = 524288 / 2;


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

fn keccak_transmute_u8(a: &mut [u64; PLEN]) -> &mut [u8; PLEN * 8] {
  unsafe { ::std::mem::transmute(a) }
}

fn keccak_transmute_u64(t: &mut [u8; TLEN]) -> &mut [u64; TLEN / 8] {
  unsafe { ::std::mem::transmute(t) }
}

// TODO probably move this keccak function to another file, and make the differences from mithril's
// version clear
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
    xorin(&mut keccak_transmute_u8(&mut a)[0..][..rate], &input[ip..]);
    keccakf(&mut a);
    ip += rate;
    l -= rate;
    rate = init_rate;
  }

  tmp[..l].copy_from_slice(&input[ip..]);
  //pad
  tmp[l] = 1;
  tmp[rate - 1] |= 0x80;

  let t64 = keccak_transmute_u64(&mut tmp);
  for i in 0..(rate/8) {
    a[i] ^= t64[i];
  }

  keccakf(&mut a);

  let t8 = keccak_transmute_u8(&mut a);
  return *t8;
}

/// This is mainly for testing, allocates a new scratchpad on every hash
pub fn hash_alloc_scratchpad(input: &[u8], aes: &AES) -> String {
  let mut scratchpad : Box<[u64x2; MEM_SIZE]> = box [u64x2(0,0); MEM_SIZE];
  return hash(&mut scratchpad, input, aes);
}

pub fn hash(mut scratchpad : &mut Box<[u64x2; MEM_SIZE]>, input: &[u8], aes: &AES) -> String {
  //scratchpad init
  let mut state = keccak(input);
  init_scratchpad(&mut scratchpad, &mut state, &aes);

  let mut a = u64x2::read(&state[0..16]) ^ u64x2::read(&state[32..48]);
  let mut b = u64x2::read(&state[16..32]) ^ u64x2::read(&state[48..64]);

  let mut i = 0;
  while i < ITERATIONS {
    let mut ix = scratchpad_addr(&a);
    let aes_result = aes.aes_round(scratchpad[ix], a);
    scratchpad[ix] = b ^ aes_result;

    ix = scratchpad_addr(&aes_result);
    let mem = scratchpad[ix];
    let add_r = ebyte_add(&a, &ebyte_mul(&aes_result, &mem));
    scratchpad[ix] = add_r;

    a = add_r ^ mem;
    b = aes_result;

    i += 1;
  }

  let final_result = finalise_scratchpad(scratchpad, &mut state, &aes);

  let mut k = 0;
  while k < 8 {
    let block = final_result[k];
    let offset = 64+(k<<4);
    block.write(&mut state[offset..offset+16]);
    k += 1;
  }

  let state_64 = transmute_u64(&mut state);
  keccakf(state_64);

  return final_hash(transmute_u8(state_64));
}

fn final_hash(keccak_state: &[u8; 200]) -> String {
  let hash_result = match keccak_state[0] & 3 {
    0 => {
      let mut result = [0; 32];
      blake::hash(256, keccak_state, &mut result).unwrap();
      byte_string::u8_array_to_string(&result)
    },
    1 => {
      let mut hasher = Groestl256::default();
      hasher.input(keccak_state);
      format!("{:x}", hasher.result())
    },
    2 => {
      let mut result = [0; 32];
      jhffi::hash(256, keccak_state, &mut result).unwrap();
      byte_string::u8_array_to_string(&result)
    },
    3 => {
      let mut result = [0; 32];
      skeinffi::hash(256, keccak_state, &mut result).unwrap();
      byte_string::u8_array_to_string(&result)
    },
    _ => panic!("hash select error")
  };
  return hash_result;
}

fn transmute_u64(t: &mut [u8; 200]) -> &mut [u64; 25] {
  unsafe { ::std::mem::transmute(t) }
}

fn transmute_u8(t: &mut [u64; 25]) -> &mut [u8; 200] {
  unsafe { ::std::mem::transmute(t) }
}

pub fn ebyte_mul(a: &u64x2, b: &u64x2) -> u64x2 {
  let r0 = u128::from(a.0);
  let r1 = u128::from(b.0);
  let r = r0 * r1;
  return u64x2((r >> 64) as u64, r as u64);
}

pub fn ebyte_add(a: &u64x2, b: &u64x2) -> u64x2 {
  return u64x2(a.0.wrapping_add(b.0), a.1.wrapping_add(b.1));
}

pub fn scratchpad_addr(u: &u64x2) -> usize {
  return ((u.0 & 0xFFFF0) >> 4) as usize;
}

pub fn finalise_scratchpad(scratchpad: &mut Box<[u64x2; MEM_SIZE]>, keccak_state: &mut [u8; 200], aes: &AES) -> [u64x2; 8] {
  let t_state = transmute_u64(keccak_state);
  let input0 = u64x2(t_state[4], t_state[5]);
  let input1 = u64x2(t_state[6], t_state[7]);

  let keys = aes.gen_round_keys(input0, input1);

  let mut state : [u64x2; 8] = [u64x2(0,0); 8];
  let mut i = 0;
  while i < 8 {
    let offset = i*2;
    let mut block = u64x2(t_state[8+offset], t_state[8+offset+1]);
    block = scratchpad[i] ^ block;
    let mut k = 0;
    while k < 10 {
      block = aes.aes_round(block, keys[k]);
      k += 1;
    }
    state[i] = block;
    i += 1;
  }

  let mut k = 8;
  while k < MEM_SIZE {
    let mut i = 0;
    while i < 8 {
      let mut block = scratchpad[k+i];
      block = state[i] ^ block;
      let mut j = 0;
      while j < 10 {
        block = aes.aes_round(block, keys[j]);
        j += 1;
      }
      state[i] = block;
      i += 1;
    }
    k += 8;
  }
  return state;
}

pub fn init_scratchpad(scratchpad : &mut Box<[u64x2; MEM_SIZE]>, state: &mut [u8; 200], aes: &AES) {
  let t_state = transmute_u64(state);
  let input0 = u64x2(t_state[0], t_state[1]);
  let input1 = u64x2(t_state[2], t_state[3]);
  let keys = aes.gen_round_keys(input0, input1);

  let mut i = 0;
  while i < 8 {
    let offset = i*2;
    let mut block = u64x2(t_state[8+offset], t_state[8+offset+1]);
    let mut k = 0;
    while k < 10 {
      block = aes.aes_round(block, keys[k]);
      k += 1;
    }
    scratchpad[i] = block;
    i += 1;
  }

  let mut k = 0;
  while k < (MEM_SIZE-8) {
    let mut i = k;
    while i < (k+8) {
      let mut block = scratchpad[i];
      let mut j = 0;
      while j < 10 {
        block = aes.aes_round(block, keys[j]);
        j += 1;
      }
      scratchpad[i+8] = block;
      i += 1;
    }
    k += 8;
  }
}
