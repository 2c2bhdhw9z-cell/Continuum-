//! Adversarial: random and malformed cartridges must never panic the core, on the Game Boy,
//! Game Boy Color and Advance paths, and save states must round-trip from any point.
use continuum_gb::gb::{GameBoy, Model};
use continuum_gb::gba::Gba;

/// xorshift64*, seeded, so failures reproduce.
fn scale() -> usize {
    std::env::var("FUZZ_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

const CART_TYPES: [u8; 16] = [0x00, 0x01, 0x03, 0x05, 0x06, 0x0F, 0x10, 0x13, 0x19, 0x1B, 0x1E, 0x22, 0xFC, 0xFE, 0xFF, 0x77];

fn gb_rom(r: &mut Rng, size: usize) -> Vec<u8> {
    let mut rom = r.bytes(size.max(0x150));
    rom[0x147] = CART_TYPES[(r.next() % CART_TYPES.len() as u64) as usize];
    rom[0x148] = (r.next() % 12) as u8; // includes sizes larger than the file, and invalid ones
    rom[0x149] = (r.next() % 7) as u8;
    rom[0x143] = [0x00, 0x80, 0xC0, 0x42][(r.next() % 4) as usize];
    rom[0x146] = (r.next() % 4) as u8;
    rom
}

#[test]
fn random_game_boy_cartridges_never_panic() {
    let mut r = Rng(0x00C0_FFEE_1234_5678);
    let models = [Model::Auto, Model::Dmg, Model::Dmg0, Model::Mgb, Model::Sgb, Model::Sgb2, Model::Cgb, Model::Agb];
    for i in 0..300 * scale() {
        let size = [0x150, 0x3FFF, 0x8000, 0x9001, 0x20000][i % 5];
        let rom = gb_rom(&mut r, size);
        let model = models[i % models.len()];
        let Ok(mut gb) = GameBoy::with_model(rom.clone(), model, None) else { continue };
        for f in 0..6 {
            gb.run_frame();
            if f == 3 {
                let st = gb.save_state();
                let mut other = GameBoy::with_model(rom.clone(), model, None).unwrap();
                other.load_state(&st).unwrap();
                other.run_frame();
                gb.run_frame();
                // The machine state, not the picture: a state does not carry the last frame drawn
                // (a game with the LCD off keeps showing whatever was there).
                assert!(other.save_state() == gb.save_state(), "case {i} ({model:?}): state round trip diverged");
            }
        }
        // Garbage and truncated states are refused, never a panic.
        let st = gb.save_state();
        let _ = gb.load_state(&st[..st.len() / 2]);
        let junk = r.bytes(st.len());
        let _ = gb.load_state(&junk);
        // Valid header, corrupt body (huge length fields): refused, no runaway allocation.
        let mut bad = st.clone();
        for b in bad[8..].iter_mut().step_by(7) {
            *b = 0xFF;
        }
        let _ = gb.load_state(&bad);
    }
}

#[test]
fn tiny_and_empty_cartridges_are_refused_or_run() {
    for n in [0usize, 1, 0x100, 0x14F, 0x150] {
        if let Ok(mut gb) = GameBoy::new(vec![0; n]) {
            gb.run_frame();
        }
    }
}

#[test]
fn random_advance_cartridges_never_panic() {
    let mut r = Rng(0x0A0B_0C0D_0E0F_1011);
    for i in 0..40 * scale() {
        let size = [0xC0, 0x1000, 0x40000, 0x100_0001, 0x200_0000][i % 5];
        let mut rom = r.bytes(size);
        // Branch to random code half the time; also plant save-type strings.
        if i % 2 == 0 {
            rom[3] = 0xEA;
        }
        let tags: [&[u8]; 4] = [b"SRAM_V113", b"FLASH1M_V103", b"EEPROM_V124", b"FLASH512_V131"];
        let t = tags[i % 4];
        if rom.len() > 0x200 + t.len() {
            rom[0x200..0x200 + t.len()].copy_from_slice(t);
        }
        let mut g = Gba::new(rom.clone());
        for f in 0..3 {
            g.set_keys((r.next() & 0x3FF) as u16);
            g.run_frame();
            if f == 1 {
                let st = g.save_state();
                let mut h = Gba::new(rom.clone());
                h.load_state(&st).unwrap();
            }
        }
        let st = g.save_state();
        let _ = g.load_state(&st[..st.len() / 3]);
    }
}

/// The state layout is pinned: if a field is added or removed the size changes, and the format
/// tag in `save_state` must be bumped so older states are refused rather than misread.
#[test]
fn state_layout_is_pinned() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x1B; // MBC5 + RAM + battery
    rom[0x149] = 3;
    let gb = GameBoy::new(rom).unwrap();
    let st = gb.save_state();
    assert_eq!(&st[..8], b"CGBSTAT2", "bump the tag when the layout changes");
    let g = Gba::new(vec![0u8; 0x1000]);
    let sa = g.save_state();
    assert_eq!(&sa[..8], b"CGBASTA2");
    eprintln!("state sizes: gb {} gba {}", st.len(), sa.len());
    assert_eq!((st.len(), sa.len()), (GB_STATE_LEN, GBA_STATE_LEN),
               "the state layout changed: bump CGBSTAT/CGBASTA and update these sizes");
}
const GB_STATE_LEN: usize = 220_973;
const GBA_STATE_LEN: usize = 550_530;
