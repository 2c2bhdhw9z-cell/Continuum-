use continuum_gb::gb::GameBoy;
fn main() {
    for mode in ["dmg", "cgb"] {
        let data = std::fs::read("test-roms/bully/bully.gb").unwrap();
        let mut gb = if mode == "dmg" { GameBoy::new_mode(data, false).unwrap() } else { GameBoy::new_mode(data, true).unwrap() };
        println!("{mode}: DIV at start: {:#04x}", gb.bus.raw_read(0xFF04));
        for f in 0..5 {
            gb.run_frame();
            println!("{mode}: after frame {}: DIV={:#04x} cycles={}", f + 1, gb.bus.raw_read(0xFF04), gb.bus.cycles);
        }
    }
}
