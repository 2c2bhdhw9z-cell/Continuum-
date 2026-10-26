//! The Advance Wireless Adapter end to end: two Advances, each running afska's LinkWireless demo
//! (gba-link-connection v8.0.3, MIT, fetched by scripts/fetch-test-roms.sh), each with cBoy's
//! adapter on its link port, the two adapters' radio carried between their cables the way the
//! app carries a two-phone session. One serves, the other searches, finds it and connects, and
//! then the demo counts up on both, which it only does while data crosses both ways.

use std::path::PathBuf;

use continuum_gb::gba::sio::Cable;
use continuum_gb::gba::Gba;

const A: u16 = 1;
const START: u16 = 1 << 3;
const R: u16 = 1 << 8;
const L: u16 = 1 << 9;

fn rom() -> Option<Vec<u8>> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/gba-link/LinkWireless_demo.gba");
    match std::fs::read(&p) {
        Ok(r) => Some(r),
        Err(_) => {
            eprintln!("SKIPPED: {} not found (run scripts/fetch-test-roms.sh)", p.display());
            None
        }
    }
}

struct Pair {
    a: Box<Gba>,
    b: Box<Gba>,
    ca: Cable,
    cb: Cable,
}

impl Pair {
    fn frames(&mut self, n: u32, keys_a: u16, keys_b: u16) {
        for _ in 0..n {
            self.a.set_keys(keys_a);
            self.b.set_keys(keys_b);
            self.a.run_frame();
            self.b.run_frame();
            while let Some(p) = self.ca.take_out() {
                self.cb.deliver(&p);
            }
            while let Some(p) = self.cb.take_out() {
                self.ca.deliver(&p);
            }
            self.ca.pump(&mut self.a.bus);
            self.cb.pump(&mut self.b.bus);
        }
    }

    /// Presses `keys` on one console for a few frames, then lets go.
    fn press(&mut self, on_a: bool, keys: u16) {
        if on_a {
            self.frames(6, keys, 0);
        } else {
            self.frames(6, 0, keys);
        }
        self.frames(20, 0, 0);
    }
}

/// Writes a console's screen as a PNG, for a person to look at (WIRELESS_SHOTS=<folder>).
fn shot(g: &Gba, name: &str) {
    let Ok(dir) = std::env::var("WIRELESS_SHOTS") else { return };
    let (w, h) = (240usize, 160usize);
    let mut raw = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let p = g.bus.ppu.frame[y * w + x];
            raw.extend([(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }
    let file = std::fs::File::create(format!("{dir}/{name}.png")).unwrap();
    let mut enc = png::Encoder::new(file, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&raw).unwrap();
}

#[test]
fn two_advances_find_each_other_and_talk_through_their_wireless_adapters() {
    let Some(rom) = rom() else { return };
    let mut p = Pair { a: Box::new(Gba::new(rom.clone())), b: Box::new(Gba::new(rom)), ca: Cable::new(), cb: Cable::new() };
    p.ca.start(true);
    p.cb.start(false);
    p.frames(120, 0, 0);
    shot(&p.a, "0_title");
    p.press(true, A);
    p.press(false, A);
    p.frames(60, 0, 0);
    shot(&p.a, "1_menu_a");
    assert!(p.a.bus.rfu.active && p.b.bus.rfu.active, "both found their adapter");
    p.press(true, L);
    p.frames(30, 0, 0);
    shot(&p.a, "2_serving");
    p.press(false, R);
    p.frames(90, 0, 0);
    shot(&p.b, "3_search");
    p.press(false, START);
    p.frames(240, 0, 0);
    shot(&p.a, "4_host");
    shot(&p.b, "4_client");
    // The counters' lines (p0 and p1), which both screens show alike once they agree.
    let counters = |g: &Gba| g.bus.ppu.frame[64 * 240..88 * 240].to_vec();
    let before = counters(&p.b);
    assert_eq!(counters(&p.a), before, "connected, both show the same counters");
    // Holding A on the host counts its number up; the client's screen follows.
    p.frames(60, A, 0);
    p.frames(120, 0, 0);
    shot(&p.a, "5_host");
    shot(&p.b, "5_client");
    assert_ne!(counters(&p.b), before, "the host's count reached the client");
    assert_eq!(counters(&p.a), counters(&p.b), "and both agree");
    // And the other way.
    let before = counters(&p.a);
    p.frames(60, 0, A);
    p.frames(120, 0, 0);
    assert_ne!(counters(&p.a), before, "the client's count reached the host");
    assert_eq!(counters(&p.a), counters(&p.b));
}
