use continuum_gb::gb::GameBoy;
fn main() {
    for div in [0x00u16, 0x01, 0x02, 0x19, 0x26, 0xAB, 0xAC, 0xFF] {
        let data = std::fs::read("test-roms/bully/bully.gb").unwrap();
        let mut gb = GameBoy::new_mode(data, true).unwrap();
        gb.bus.timer.counter = div << 8;
        for _ in 0..30 { gb.run_frame(); }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for p in &gb.bus.ppu.frame { h = (h ^ u64::from(*p)).wrapping_mul(0x100_0000_01b3); }
        println!("DIV start {div:#04x}: hash {h:#018x}");
    }
}
