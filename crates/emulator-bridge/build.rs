fn main() {
    // Only needed when the native libretro host is compiled: that is the sole caller of the
    // GET_LOG_INTERFACE trampoline, and the only build that has a core to earn achievements in.
    // Host unit tests without `native-core` skip all of this.
    if std::env::var_os("CARGO_FEATURE_NATIVE_CORE").is_none() {
        return;
    }

    // `cargo check --target aarch64-apple-ios` from Linux is the project's iOS type-check gate,
    // and it cannot compile C for an Apple target: cc-rs needs `xcrun` to find the SDK, and the log
    // shim includes <os/log.h>, which only the Apple SDK has. Nothing is LINKED by a check, so the
    // C is skipped there and said so. A real iOS build runs on macOS, where this branch is not
    // taken, and a Linux build that tried to link an iOS binary would fail at the link anyway.
    let target_is_apple = std::env::var("CARGO_CFG_TARGET_VENDOR").is_ok_and(|v| v == "apple");
    if target_is_apple && !cfg!(target_vendor = "apple") {
        println!(
            "cargo:warning=skipping the C shims and rcheevos: an Apple target cannot be compiled \
             for from this host (fine for `cargo check`, not for a link)"
        );
        return;
    }

    println!("cargo:rerun-if-changed=src/cores/retro_log_shim.c");
    cc::Build::new()
        .file("src/cores/retro_log_shim.c")
        .warnings(true)
        .compile("continuum_retro_log_shim");

    build_rcheevos();
}

/// RetroAchievements' rcheevos (MIT), vendored at `vendor/rcheevos`, plus Continuum's flat shim
/// over its `rc_client` API (`src/achievements/rc_shim.c`).
///
/// Vendored rather than fetched here, because a build script that reaches the network is a build
/// that fails offline and on any CI runner with egress rules, and because this exact source is
/// then what was reviewed. `vendor/rcheevos/VENDORED.md` names the tag and how to update it.
///
/// The defines are upstream's own (its `Package.swift`): `RC_CLIENT_SUPPORTS_HASH` so rc_client
/// can identify a game from its file, and `RC_DISABLE_LUA`. Upstream's warnings are not ours to
/// fix, so they are off for the vendored files and on for the shim.
fn build_rcheevos() {
    let root = std::path::Path::new("vendor/rcheevos");
    println!("cargo:rerun-if-changed=vendor/rcheevos");
    println!("cargo:rerun-if-changed=src/achievements/rc_shim.c");

    let mut sources = Vec::new();
    for dir in ["src", "src/rapi", "src/rcheevos", "src/rhash"] {
        let entries = std::fs::read_dir(root.join(dir))
            .unwrap_or_else(|err| panic!("vendor/rcheevos/{dir} is missing: {err}"));
        for entry in entries {
            let path = entry.expect("readable directory entry").path();
            if path.extension().is_some_and(|ext| ext == "c") {
                sources.push(path);
            }
        }
    }
    // `rc_client_external.c` is excluded by upstream's own Package.swift: it is the bridge to an
    // external (DLL) client implementation, and is only meaningful with RC_CLIENT_SUPPORTS_EXTERNAL.
    sources.retain(|path| !path.ends_with("rc_client_external.c"));
    sources.sort();

    // The shim FIRST: it references rcheevos, and a static archive has to come before the
    // archives it depends on on a traditional linker's command line.
    cc::Build::new()
        .file("src/achievements/rc_shim.c")
        .include(root.join("include"))
        .define("RC_CLIENT_SUPPORTS_HASH", None)
        .warnings(true)
        .compile("continuum_rc_shim");

    cc::Build::new()
        .files(&sources)
        .include(root.join("include"))
        .include(root.join("src"))
        .define("RC_CLIENT_SUPPORTS_HASH", None)
        .define("RC_DISABLE_LUA", None)
        .warnings(false)
        .compile("continuum_rcheevos");
}
