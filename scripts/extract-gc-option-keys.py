#!/usr/bin/env python3
"""Collects every setting of every current GameCube / Wii emulator, for our own core's inventory.

Downloads only the settings code of each emulator (GitHub tarball at a recorded commit), pulls out
each setting's name, and writes one file per emulator to .kiro/notes/docs/option-sources/gc/:
<source>.keys (one line per setting: id TAB where it comes from) and <source>.commit.

The id is Dolphin's own name for the setting (MAIN_CPU_CORE, GFX_EFB_SCALE ...) wherever the
emulator is Dolphin-based, so the same setting lines up across emulators. A setting with no
Dolphin name keeps its own key (dolphin_xxx for the RetroArch core, ANDROID_xxx for the Android
app's own settings).

Nothing downloaded here is ever committed or shipped: only the lists of names are. Usage:
    scripts/extract-gc-option-keys.py [workdir]
"""
import glob
import json
import os
import re
import subprocess
import sys
import tarfile
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, ".kiro/notes/docs/option-sources/gc")
WORK = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "..", ".scratch", "gc-option-sources")

# name, repo, branch, what it is
SOURCES = [
    ("dolphin", "dolphin-emu/dolphin", "master", "Dolphin (computer and Android app)"),
    ("libretro", "libretro/dolphin", "master", "Dolphin for RetroArch"),
    ("manic", "Daiuno/dolphin", "manicInterpreter", "Manic EMU's Dolphin"),
    ("dolphinios", "OatmealDome/dolphin-ios", "master", "DolphiniOS"),
    ("icube", "Provenance-Emu/dolphin-ios-jitless", "develop", "iCube (Provenance's Dolphin for iOS)"),
    ("provenance", "Provenance-Emu/Provenance", "develop", "Provenance (its Dolphin settings)"),
    ("mmjr2", "Medard22/Dolphin-MMJR2-VBI", "master", "Dolphin MMJR2 (Android)"),
    ("slippi", "project-slippi/dolphin", "slippi", "Slippi Dolphin"),
    ("primehack", "shiiion/dolphin", "primehack-master", "PrimeHack"),
    ("mpn", "MarioPartyNetplay/Dolphin-MPN", "master", "Dolphin MPN (Mario Party Netplay)"),
    ("ishiiruka", "project-slippi/Ishiiruka", "slippi", "Ishiiruka (Slippi's older Dolphin)"),
    ("dolphinps4", "iHaiDeeZ/DolphinPS4", "main", "DolphinPS4 (Dolphin for PS4)"),
    ("porpoise", "elripalda/Porpoise-Dolphin-Emulator-for-PS5", "Main", "Porpoise (Dolphin for PS5)"),
    # Not Dolphin: written from scratch.
    ("lazuli", "vxpm/lazuli", "main", "Lazuli (Rust, from scratch)"),
    ("gecko", "ioncodes/gecko", "master", "gecko (Rust, from scratch)"),
    ("rustcube", "msierks/rustcube", "master", "rustcube (Rust, from scratch)"),
    ("pureikyubu", "emu-russia/pureikyubu", "master", "pureikyubu (C++, from scratch)"),
]

# Emulators that are not Dolphin: how their settings are read (see their extractors below).
INDEPENDENT = ("lazuli", "gecko", "rustcube", "pureikyubu")

WANTED = (
    "/Source/Core/Core/Config/",
    "/Source/Core/DolphinLibretro/",
    "/Source/Android/app/src/main/java/org/dolphinemu/dolphinemu/features/settings/model/",
    "/Cores/Dolphin/PVDolphinCore/Core/PVDolphinCore",
    "/Source/Core/Core/ConfigManager.cpp",
    "/Source/Core/VideoCommon/VideoConfig.cpp",
    # DolphinPS4 and Porpoise keep their own changes as patches and launcher code.
    "/patches/", "/src/", "/config/",
    # Lazuli, gecko, rustcube, pureikyubu.
    "/crates/app/src/cli.rs", "/crates/app/src/config.rs", "/crates/hostinput/src/config.rs",
    "/build/Data/DefaultSettings.json",
)
SMALL = ("lazuli", "gecko", "rustcube", "pureikyubu", "dolphinps4", "porpoise")


def gh(path):
    return json.loads(subprocess.check_output(["gh", "api", path]))


def fetch(name, repo, branch):
    sha = gh(f"repos/{repo}/commits/{branch}")["sha"]
    dest = os.path.join(WORK, name)
    stamp = os.path.join(dest, ".sha")
    if os.path.exists(stamp) and open(stamp).read() == sha:
        return sha, dest
    os.makedirs(dest, exist_ok=True)
    url = f"https://codeload.github.com/{repo}/tar.gz/{sha}"
    with urllib.request.urlopen(url) as resp, tarfile.open(fileobj=resp, mode="r|gz") as tar:
        for member in tar:
            path = "/" + member.name.split("/", 1)[-1]
            if not member.isfile():
                continue
            if name in SMALL and not (path.endswith((".rs", ".json", ".ini", ".patch", ".diff", ".cpp", ".h"))):
                continue
            if name not in SMALL and path.startswith(("/patches/", "/src/", "/config/")):
                continue
            if any(w in path for w in WANTED) or (name == "rustcube" and path == "/src/main.rs"):
                member.name = path.lstrip("/")
                tar.extract(member, dest)
    open(stamp, "w").write(sha)
    return sha, dest


INFO = re.compile(
    r'Info<([^;{]*?)>\s+(\w+)\s*\{\s*\{\s*System::(\w+)\s*,\s*([^,]+?)\s*,\s*([^}]+?)\s*\}', re.S)


def dolphin_infos(dest):
    """Dolphin's settings: every `const Info<T> NAME{{System::X, "Section", "Key"}, default}`."""
    found = {}
    for path in sorted(glob.glob(os.path.join(dest, "Source/Core/Core/Config/*.cpp"))):
        for m in INFO.finditer(open(path, errors="ignore").read()):
            section = m.group(4).strip().strip('"')
            key = m.group(5).strip().strip('"')
            found[m.group(2)] = f"{m.group(3)}/{section}/{key}"
    return found


def libretro_options(dest, infos):
    """RetroArch options (dolphin_xxx), each tied to the Dolphin setting it sets where it sets one."""
    base = os.path.join(dest, "Source/Core/DolphinLibretro")
    header = open(os.path.join(base, "Common/Options.h"), errors="ignore").read()
    consts = {}  # namespace::NAME -> key
    ns = ""
    for line in header.splitlines():
        m = re.match(r'\s*namespace (\w+)', line)
        if m:
            ns = m.group(1)
        for const, key in re.findall(r'constexpr const char (\w+)\[\]\s*=\s*"(dolphin_\w+)"', line):
            consts[f"{ns}::{const}"] = key
    keys = set(consts.values())
    # Keys written straight into the code rather than through a constant.
    for path in glob.glob(os.path.join(base, "**/*.cpp"), recursive=True):
        keys |= set(re.findall(r'"(dolphin_[a-z0-9_]+)"', open(path, errors="ignore").read()))
    tied = {}
    for path in glob.glob(os.path.join(base, "**/*.cpp"), recursive=True):
        for stmt in open(path, errors="ignore").read().split(";"):
            targets = re.findall(r'Config::((?:MAIN|GFX|SYSCONF|WIIMOTE|FREE_LOOK|RA|NETPLAY|SESSION)_\w+)', stmt)
            used = [consts[c] for c in re.findall(r'(\w+::[A-Z0-9_]+)\b', stmt) if c in consts]
            if len(set(used)) == 1 and targets:
                for t in targets:
                    if t in infos:
                        tied.setdefault(used[0], set()).add(t)
    rows = []
    for key in sorted(keys):
        ids = sorted(tied.get(key, [])) or [key]
        rows += [(i, key) for i in ids]
    return rows


ANDROID = re.compile(r'^\s+([A-Z][A-Z0-9_]+)\(\s*Settings\.(FILE_\w+)\s*,\s*(?:Settings\.)?(\w+|"[^"]*")\s*,\s*"([^"]+)"', re.M)


def android_settings(dest, infos):
    """The Android app's settings list. Matched to Dolphin's setting by name, else its own id."""
    by_key = {}
    for name, where in infos.items():
        by_key.setdefault(where.split("/")[-1], name)
    rows = []
    folder = os.path.join(dest, "Source/Android/app/src/main/java/org/dolphinemu/dolphinemu/features/settings/model")
    for path in sorted(glob.glob(os.path.join(folder, "*Setting.kt")) + glob.glob(os.path.join(folder, "*Setting.java"))):
        for enum, file_, section, key in ANDROID.findall(open(path, errors="ignore").read()):
            if enum in infos:
                rows.append((enum, f"{os.path.basename(path)}:{key}"))
            elif key in by_key and file_ != "FILE_DOLPHIN_ANDROID" and "ANDROID" not in section:
                rows.append((by_key[key], f"{os.path.basename(path)}:{key}"))
            else:
                rows.append((f"ANDROID_{enum}", f"{os.path.basename(path)}:{section.strip(chr(34))}/{key}"))
    return rows


def provenance_settings(dest, infos):
    """Provenance: the Dolphin settings its core sets from its own options."""
    rows = set()
    by_key = {where.split("/")[-1]: name for name, where in infos.items()}
    for path in glob.glob(os.path.join(dest, "Cores/Dolphin/PVDolphinCore/Core/PVDolphinCore*")):
        text = open(path, errors="ignore").read()
        for name in re.findall(r'Config::Set\w*\(\s*Config::(\w+)', text):
            rows.add((name, os.path.basename(path)))
        for key in re.findall(r'key:\s*"(\w+)"', text):
            rows.add((by_key.get(key, f"PROVENANCE_{key}"), f"{os.path.basename(path)}:{key}"))
    return sorted(rows)


def ishiiruka_settings(dest):
    """Older Dolphin (before Config::Info): `section->Get("Key", ...)` in ConfigManager and VideoConfig."""
    rows = []
    for rel in ("Source/Core/Core/ConfigManager.cpp", "Source/Core/VideoCommon/VideoConfig.cpp"):
        path = os.path.join(dest, rel)
        if os.path.exists(path):
            for section, key in re.findall(r'(\w+)->Get\(\s*"(\w+)"', open(path, errors="ignore").read()):
                rows.append((f"ISHIIRUKA_{key}", f"{os.path.basename(rel)}:{section}/{key}"))
    return rows


def port_settings(dest, base_infos):
    """Console ports of Dolphin: their own added Infos and RetroArch-style keys, beyond the base."""
    rows = []
    for path in glob.glob(os.path.join(dest, "**/*"), recursive=True):
        if not os.path.isfile(path):
            continue
        text = open(path, errors="ignore").read()
        rel = os.path.relpath(path, dest)
        for m in INFO.finditer(text):
            if m.group(2) not in base_infos:
                rows.append((m.group(2), rel))
        for key in set(re.findall(r'"(dolphin_[a-z0-9_]+)"', text)):
            rows.append((key, rel))
    return rows


def independent_settings(name, dest):
    """Emulators written from scratch: each setting under its own name (mapped by hand)."""
    rows = []
    def struct_fields(rel, struct=None):
        path = os.path.join(dest, rel)
        if not os.path.exists(path):
            return
        text = open(path, errors="ignore").read()
        blocks = re.findall(r'pub struct (\w+)\s*\{(.*?)\n\}', text, re.S)
        for sname, body in blocks:
            if struct and sname not in struct:
                continue
            for field in re.findall(r'^\s*pub (\w+):', body, re.M):
                rows.append((f"{name.upper()}_{sname}.{field}", rel))
    if name == "lazuli":
        struct_fields("crates/app/src/cli.rs")
    elif name == "gecko":
        struct_fields("crates/app/src/config.rs", ("Config", "WiiPreset", "WiiPresets"))
        struct_fields("crates/hostinput/src/config.rs")
    elif name == "rustcube":
        path = os.path.join(dest, "src/main.rs")
        for long in re.findall(r'opts\.opt\w+\(\s*"[^"]*"\s*,\s*"([^"]+)"', open(path, errors="ignore").read()):
            rows.append((f"RUSTCUBE_{long}", "src/main.rs"))
    elif name == "pureikyubu":
        data = json.load(open(os.path.join(dest, "build/Data/DefaultSettings.json")))
        for section, values in data.items():
            if section in ("info",) or not isinstance(values, dict):
                if section == "io":
                    for dev in values.get("Devices", []):
                        rows.append((f"PUREIKYUBU_io.{dev['Name'].replace(' ', '_')}", "DefaultSettings.json"))
                continue
            for key in values:
                rows.append((f"PUREIKYUBU_{section}.{key}", "build/Data/DefaultSettings.json"))
    return rows


def write(name, repo, branch, sha, label, rows):
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, f"{name}.commit"), "w") as f:
        f.write(f"{sha}\nhttps://github.com/{repo} ({branch})\n{label}\n")
    with open(os.path.join(OUT, f"{name}.keys"), "w") as f:
        for i, where in sorted(set(rows)):
            f.write(f"{i}\t{where}\n")
    print(f"{name}: {len(set(r[0] for r in rows))} settings at {sha[:9]}")


def main():
    os.makedirs(WORK, exist_ok=True)
    fetched = {name: fetch(name, repo, branch) for name, repo, branch, _ in SOURCES}
    infos_all = {}
    for name, repo, branch, label in SOURCES:
        sha, dest = fetched[name]
        infos = dolphin_infos(dest)
        infos_all.update(infos)
        rows = [(i, w) for i, w in infos.items()]
        if name == "provenance":
            continue
        if name in INDEPENDENT:
            write(name, repo, branch, sha, label, independent_settings(name, dest))
            continue
        if name == "ishiiruka":
            write(name, repo, branch, sha, label, ishiiruka_settings(dest))
            continue
        if name in ("dolphinps4", "porpoise"):
            write(name, repo, branch, sha, label + ": only what it adds to Dolphin",
                  port_settings(dest, infos_all))
            continue
        if name in ("libretro", "manic"):
            # Inside RetroArch only the options are settable, not Dolphin's own settings file.
            rows = libretro_options(dest, infos)
        write(name, repo, branch, sha, label, rows)
        if name in ("dolphin", "mmjr2"):
            app = "dolphin_android" if name == "dolphin" else "mmjr2_android"
            write(app, repo, branch, sha, label + ", Android settings screens",
                  android_settings(dest, infos))
    name, repo, branch, label = SOURCES[5]
    sha, dest = fetched[name]
    write(name, repo, branch, sha, label, provenance_settings(dest, infos_all))
    with open(os.path.join(OUT, "names.tsv"), "w") as f:
        f.write("# Dolphin setting name\twhere Dolphin keeps it (system/section/key)\n")
        for i in sorted(infos_all):
            f.write(f"{i}\t{infos_all[i]}\n")


if __name__ == "__main__":
    main()
