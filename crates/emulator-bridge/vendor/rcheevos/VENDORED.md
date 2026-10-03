# rcheevos, vendored

Upstream: https://github.com/RetroAchievements/rcheevos, tag `v12.5.0`, MIT (see `LICENSE`).

Copied unmodified from the release tarball: `include/` and `src/`. Removed, because nothing here
uses them: `src/rc_libretro.c` and `.h` (they need `libretro.h` at compile time; Continuum maps
core memory itself in `src/achievements/memory_map.rs`), the RAIntegration files (Windows only),
`.natvis` debugger files and the Swift package module map.

Compiled by `crates/emulator-bridge/build.rs` with the `cc` crate when the `native-core` feature
is on, with `RC_CLIENT_SUPPORTS_HASH` and `RC_DISABLE_LUA`, the same defines upstream's own
`Package.swift` uses. To update: replace `include/` and `src/` from a newer tag, delete the same
files again, and bump the tag above.
