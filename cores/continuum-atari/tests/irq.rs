//! A cartridge whose header wires its POKEY's interrupt to the processor gets it: a test program
//! starts timer 1, waits with interrupts allowed, and counts how many arrive. Without the header's
//! bit, none do (as on MAME and ProSystem, which never connect it).

use continuum_atari::cart::Cart;
use continuum_atari::machine::{Atari7800, Region};

/// A tiny program at $8000: POKEY timer 1 every 256 x 28 clocks (64 kHz, AUDF1 = $FF), its
/// interrupt enabled, `cli`, then a loop. The IRQ handler counts in $40 and acknowledges.
fn program() -> Vec<u8> {
    let code: &[u8] = &[
        0x78, 0xD8, 0xA2, 0xFF, 0x9A, // sei cld ldx #$FF txs
        0xA9, 0x00, 0x85, 0x40, // lda #0 sta $40
        0xA9, 0x03, 0x8D, 0x0F, 0x40, // lda #3 sta SKCTL: counters run
        0xA9, 0xFF, 0x8D, 0x00, 0x40, // lda #$FF sta AUDF1
        0xA9, 0x01, 0x8D, 0x0E, 0x40, // lda #1 sta IRQEN
        0x8D, 0x09, 0x40, // sta STIMER
        0x58, // cli
        0x4C, 0x1C, 0x80, // loop: jmp loop
        // irq at $801F:
        0xE6, 0x40, // inc $40
        0x48, 0xA9, 0x00, 0x8D, 0x0E, 0x40, // pha lda #0 sta IRQEN (acknowledge)
        0xA9, 0x01, 0x8D, 0x0E, 0x40, 0x68, // lda #1 sta IRQEN pla
        0x40, // rti
    ];
    let mut rom = vec![0xFFu8; 0x8000];
    rom[..code.len()].copy_from_slice(code);
    rom[0x7FFA..].copy_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x1F, 0x80]);
    rom
}

fn irqs_in_a_frame(wired: bool) -> u8 {
    let rom = program();
    let mut h = vec![0u8; 128];
    h[0] = 3;
    h[1..10].copy_from_slice(b"ATARI7800");
    h[49..53].copy_from_slice(&(rom.len() as u32).to_be_bytes());
    h[53..55].copy_from_slice(&1u16.to_be_bytes()); // a POKEY at $4000
    h[62] = if wired { 1 } else { 0 };
    h.extend(rom);
    let mut m = Atari7800::new(Cart::load(&h).unwrap(), Region::Ntsc, None);
    m.run_frame();
    let before = m.sys.ram[0x840];
    m.run_frame();
    m.sys.ram[0x840].wrapping_sub(before)
}

#[test]
fn a_wired_pokey_interrupts_the_processor() {
    // 1.79 MHz / 28 / 256 = 249.7 a second, about 4.2 a frame.
    let n = irqs_in_a_frame(true);
    assert!((4..=5).contains(&n), "{n} interrupts in a frame");
    assert_eq!(irqs_in_a_frame(false), 0, "not wired, none arrive");
}
