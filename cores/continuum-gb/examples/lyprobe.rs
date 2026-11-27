use continuum_gb::gb::GameBoy;
fn main() {
    let data = std::fs::read("test-roms/little-things-gb/tellinglys.gb").unwrap();
    let mut gb = GameBoy::new(data).unwrap();
    for _ in 0..30 { gb.run_frame(); }
    println!("after 30 frames: ly={} pc={:04X} halted={}", gb.bus.ppu.ly, gb.cpu.pc, gb.cpu.halted);
    for _ in 0..200_000 {
        if gb.bus.ppu.ly == 10 { break; }
        gb.cpu.step(&mut gb.bus);
    }
    println!("at LY10 wait end: ly={} pc={:04X} halted={}", gb.bus.ppu.ly, gb.cpu.pc, gb.cpu.halted);
    gb.bus.set_buttons(1);
    println!("pressed: if={:#04x}", gb.bus.iflag);
    for i in 0..10 {
        for _ in 0..1000 { gb.cpu.step(&mut gb.bus); }
        println!("+{}k steps: ly={} pc={:04X} halted={} if={:#04x}", (i+1), gb.bus.ppu.ly, gb.cpu.pc, gb.cpu.halted, gb.bus.iflag);
    }
}
