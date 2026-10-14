//! Continuum's own 32X start-up code as `scripts/patches/blastem-32x-startup.patch` ships it: its
//! source, `continuum_32x_boot.s`, and the bytes BlastEm compiles in, `continuum_32x_boot.h`.
//!
//! Always run: both images are in the header, fit the 16 KB boot ROM window, and start where an
//! SH-2 coming out of reset looks (the reset vector points into the image's own code). With
//! `SH_ELF_BIN` set to a folder holding GNU binutils for sh-elf (`sh-elf-as`, `sh-elf-ld`,
//! `sh-elf-objcopy`), the test also assembles the source with the commands at its top and checks
//! the header holds exactly those bytes, so the shipped bytes are the shipped source.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A file the patch creates, as its lines (without the leading `+`).
fn new_file(name: &str) -> String {
    let patch = std::fs::read_to_string(repo().join("scripts/patches/blastem-32x-startup.patch"))
        .expect("scripts/patches/blastem-32x-startup.patch");
    let start = patch
        .find(&format!("+++ b/{name}\n"))
        .unwrap_or_else(|| panic!("{name} is not in the patch"));
    let body = &patch[start..];
    let body = &body[body.find("\n@@").expect("a hunk") + 1..];
    let body = &body[body.find('\n').unwrap() + 1..];
    let mut text = String::new();
    for line in body.lines() {
        let Some(added) = line.strip_prefix('+') else { break };
        text.push_str(added);
        text.push('\n');
    }
    text
}

/// The bytes of one array in the header.
fn image(header: &str, name: &str) -> Vec<u8> {
    let start = header
        .find(&format!("static const uint8_t {name}["))
        .unwrap_or_else(|| panic!("{name} is not in the header"));
    let body = &header[start..];
    let body = &body[body.find('{').unwrap() + 1..body.find("};").unwrap()];
    body.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| u8::from_str_radix(s.trim_start_matches("0x"), 16).unwrap())
        .collect()
}

#[test]
fn both_images_start_where_an_sh2_looks() {
    let header = new_file("continuum_32x_boot.h");
    for name in ["continuum_32x_master_boot", "continuum_32x_slave_boot"] {
        let bytes = image(&header, name);
        assert!(bytes.len() > 0x400 && bytes.len() <= 0x4000, "{name}: {} bytes", bytes.len());
        // Big-endian like the console's own files: the power-on PC, then the power-on SP.
        let pc = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;
        assert!(pc >= 0x400 && pc < bytes.len() && pc % 2 == 0, "{name}: reset vector {pc:#X}");
        assert_eq!(header.matches(&format!("[sizeof({name}) <= 0x4000 ? 1 : -1]")).count(), 1, "{name}");
    }
}

#[test]
fn the_bytes_are_the_source_assembled() {
    let Ok(bin) = std::env::var("SH_ELF_BIN") else {
        eprintln!("SH_ELF_BIN not set: skipped (point it at GNU binutils built for sh-elf)");
        return;
    };
    let bin = Path::new(&bin);
    let work = std::env::temp_dir().join(format!("continuum-32x-boot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let source = work.join("continuum_32x_boot.s");
    std::fs::write(&source, new_file("continuum_32x_boot.s")).unwrap();
    let header = new_file("continuum_32x_boot.h");
    let run = |tool: &str, args: &[&str]| {
        let out = Command::new(bin.join(tool)).args(args).current_dir(&work).output().unwrap();
        assert!(out.status.success(), "{tool} {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    for (cpu, name) in [("master", "continuum_32x_master_boot"), ("slave", "continuum_32x_slave_boot")] {
        let (obj, elf, raw) = (format!("{cpu}.o"), format!("{cpu}.elf"), format!("{cpu}.bin"));
        let mut args = vec!["--isa=sh2", "-big"];
        if cpu == "slave" {
            args.extend(["--defsym", "SLAVE=1"]);
        }
        args.extend(["-o", &obj, "continuum_32x_boot.s"]);
        run("sh-elf-as", &args);
        run("sh-elf-ld", &["-EB", "-Ttext=0", "-e", "0", "-o", &elf, &obj]);
        run("sh-elf-objcopy", &["-O", "binary", "-j", ".text", &elf, &raw]);
        let assembled = std::fs::read(work.join(&raw)).unwrap();
        assert_eq!(image(&header, name), assembled, "{name} is not what continuum_32x_boot.s assembles to");
    }
    let _ = std::fs::remove_dir_all(&work);
}
