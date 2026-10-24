//! Prints the JIT's ARM64 code for a block of PowerPC instruction words (hex), for llvm-mc.
use continuum_gc::jit::compile::{compile, Inst};
fn main() {
    let words: Vec<u32> = std::env::args().skip(1).map(|s| u32::from_str_radix(&s, 16).unwrap()).collect();
    let ops: Vec<_> = words.iter().map(|&w| continuum_gc::cpu::decode_for_tools(w)).collect();
    let insts: Vec<Inst> = ops.iter().enumerate().map(|(k, op)| Inst { ea: 0x8000_3100 + 4 * k as u32, op }).collect();
    let c = compile(&insts, true);
    for w in c.code { let b = w.to_le_bytes(); println!("0x{:02x} 0x{:02x} 0x{:02x} 0x{:02x}", b[0], b[1], b[2], b[3]); }
}
