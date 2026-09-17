//! The UniFFI binding generator for this workspace.
//!
//! Invoked by `native/ios/build-engine.sh` as:
//!
//! ```text
//! uniffi-bindgen generate --library <libemulator_bridge.dylib> --language swift --out-dir <dir>
//! ```
//!
//! Library mode is used rather than a UDL file because the interface is declared with
//! `#[uniffi::export]` proc-macros in `emulator-bridge`, so the metadata lives in the
//! compiled artefact. Note that it must be pointed at the **dylib**, never the staticlib:
//! UniFFI's `calc_cdylib_name` only recognises `.so`, `.dll` and `.dylib`.

fn main() {
    uniffi::uniffi_bindgen_main()
}
