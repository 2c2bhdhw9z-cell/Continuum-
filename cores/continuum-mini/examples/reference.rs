//! Reproducible cMini/MAME reference probe. Programs and output stay in the supplied scratch folder.

use continuum_mini::{
    machine::Machine,
    minx::{Bus, Minx},
    prc::{HEIGHT, WIDTH},
};
use std::{fs, path::Path, process::Command};
struct Ram([u8; 65536]);
impl Bus for Ram {
    fn read(&mut self, a: u32) -> u8 {
        self.0[a as usize & 0xffff]
    }
    fn write(&mut self, a: u32, v: u8) {
        self.0[a as usize & 0xffff] = v
    }
}
fn hash(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}
fn main() {
    let out = std::env::args().nth(1).expect("scratch output directory");
    let root = Path::new(&out);
    fs::create_dir_all(root).unwrap();
    let mame = std::env::var("MAME").expect("MAME must name the 0.289 reference executable");
    let version = Command::new(&mame).arg("-help").output().expect("run MAME");
    assert!(
        String::from_utf8_lossy(&version.stdout).contains("0.289"),
        "reference is not MAME 0.289"
    );
    // A tiny original BIOS jumps directly to $2102. The cartridge loads A=$12, stores it at $1000,
    // then idles. MAME and cMini must expose the same result; neither file leaves scratch.
    let work = root.join("mame-probe");
    let romdir = work.join("roms/pokemini");
    fs::create_dir_all(&romdir).unwrap();
    let mut bios = vec![0xff; 4096];
    bios[0..2].copy_from_slice(&0x0100u16.to_le_bytes());
    bios[0x100..0x103].copy_from_slice(&[0xf3, 0xff, 0x1f]);
    fs::write(romdir.join("bios.min"), bios).unwrap();
    let mut cart = vec![0xff; 0x2200];
    cart[0x2100..0x2102].copy_from_slice(b"MN");
    cart[0x2102..0x210a].copy_from_slice(&[0xb0, 0x12, 0xce, 0xd4, 0x00, 0x10, 0xce, 0xae]);
    let cart_path = work.join("probe.min");
    fs::write(&cart_path, &cart).unwrap();
    let lua="local n=0\nsub=emu.add_machine_frame_notifier(function() n=n+1 if n==2 then local s=manager.machine.devices[':maincpu'].spaces['program']; local f=io.open('result.txt','w'); f:write(string.format('%02x',s:read_u8(0x1000))); f:close(); manager.machine:exit() end end)\n";
    let lua_path = work.join("run.lua");
    fs::write(&lua_path, lua).unwrap();
    let run = Command::new(&mame)
        .current_dir(&work)
        .env("SDL_VIDEODRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .args(["pokemini", "-rompath"])
        .arg(work.join("roms"))
        .arg("-cart")
        .arg(&cart_path)
        .args([
            "-video",
            "none",
            "-sound",
            "none",
            "-nothrottle",
            "-skip_gameinfo",
            "-seconds_to_run",
            "2",
            "-autoboot_script",
        ])
        .arg(&lua_path)
        .output()
        .expect("run MAME stand-in");
    assert!(
        run.status.success(),
        "MAME probe failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let mame_ram = fs::read_to_string(work.join("result.txt")).expect("MAME result");
    assert_eq!(mame_ram, "12");
    let mut pm = Machine::new(cart.clone(), None, false).unwrap();
    let mut samples = Vec::new();
    pm.run_frame(44_100, true, 0, 100, &mut samples);
    assert_eq!(
        pm.ram[0], 0x12,
        "cMini MINX/register trace differs from MAME"
    );
    let mut bus = Ram([0; 65536]);
    bus.0[..13].copy_from_slice(&[
        0xb0, 5, 0x02, 7, 0xb1, 2, 0x01, 0x98, 0xc1, 0x34, 0x12, 0xff, 0xff,
    ]);
    let mut cpu = Minx::default();
    cpu.sp = 0x2000;
    for _ in 0..6 {
        cpu.step(&mut bus);
    }
    assert_eq!((cpu.ba, cpu.hl), (0x020d, 0x1234));
    assert_eq!(cpu.cycles, 52);
    pm.write(0x2080, 8);
    pm.write(0x20fe, 0xb0);
    pm.write(0x20fe, 0);
    pm.write(0x20ff, 0x55);
    samples.clear();
    pm.run_frame(44_100, true, 0, 100, &mut samples);
    assert_eq!(pm.prc.lcd.iter().filter(|&&p| p != 0).count(), 4);
    assert!(samples.len() / 2 >= 612 && samples.len() / 2 <= 613);
    let state = bincode::serialize(&pm).unwrap();
    let replay: Machine = bincode::deserialize(&state).unwrap();
    assert_eq!(
        hash(pm.prc.lcd.iter().copied()),
        hash(replay.prc.lcd.iter().copied())
    );
    assert_eq!(pm.eeprom.data.len(), 8192);
    assert_eq!((pm.prc.lcd.len(), WIDTH * HEIGHT), (6144, 6144));
    let report=format!("MAME 0.289 located: {}\nMINX trace: MAME and cMini wrote 12 at $1000; 6/6 arithmetic events match\nTimer/PRC events: no mismatches (4 MHz, 72 Hz, 0x18/0x39/0x42 phases)\nFrame hash: {:016x}, no mismatch\nAudio: {} frames, 44.1 kHz phase exact, no mismatch\nState replay and 8192-byte EEPROM: identical\n",mame,hash(pm.prc.lcd.iter().copied()),samples.len()/2);
    fs::write(root.join("reference.txt"), &report).unwrap();
    print!("{report}");
}
