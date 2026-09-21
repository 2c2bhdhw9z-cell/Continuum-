# Product scope: the .ipa is the only deliverable

## There is no PWA, and there never will be

Continuum ships as **one artefact: a sideloadable iOS `.ipa`**. It is an all-in-one emulator
for iPhone, with JIT and the increased-memory entitlements.

Do not name a specific installer in documentation. The owner does not use TrollStore, and
install instructions must stay installer-neutral.

The web app under `web/` was scaffolding to prove the Rust engine worked before anything could
be compiled for iOS. **That job finished and the web app has been deleted**, along with
`.github/workflows/deploy.yml`, the wasm facade (`src/wasm.rs`), the wasm core loader, the
wasm host shim, `core-shim/`, the Node test scripts, and the wasm half of
`scripts/build-core.sh`. The crate no longer builds for `wasm32` and that is intentional, so
do not add a wasm target check back as a CI gate.

Rules that follow, and they are not negotiable:

- Do not reintroduce a web target, a PWA, a browser build or a GitHub Pages deploy. Not as a
  demo, not as a test harness, not "just to check the engine".
- Do not describe the project as having a web half and an iOS half. It has one product.
- "Done" for this project means: the `.ipa` plays every supported system, and the only
  remaining work is steady updates for users of that `.ipa`.

## Android is a real future deliverable, and it is not a PWA

An Android `.apk` is planned **after** iOS is complete. It is a third facade over the same
Rust engine, reached through UniFFI's Kotlin bindings, in the same way `uniffi_api.rs` serves
Swift. It is emphatically not a web wrapper.

The owner's own device is an iPhone, so the `.ipa` is updated often and the `.apk` will be
updated occasionally. Anything added to the engine should therefore stay platform-neutral:
put behaviour in the Rust engine rather than in Swift wherever there is a choice, because
everything in Swift is work that Android will have to pay for a second time. Volume is the
worked example, applied in `bridge.rs` rather than on the iOS mixer for exactly this reason.

**Shared engine, native look. These are not the same decision and must not be unified.** The
owner stated it directly about the segmented selectors: iOS gets the glass appearance, a
recessed groove with a translucent floating thumb, and the flat filled-rectangle style is the
right one for Android because that is Material Design's language. So a control's BEHAVIOUR
belongs in the engine or in shared reasoning, while its APPEARANCE is per platform and a
difference between the two builds is correct rather than drift. `SegmentedChoice` in
`native/ios/SettingsScreen.swift` carries the long-form note; do not "fix" it into a filled
block for consistency with a future Android screen.

## What stays

The Rust engine (`crates/emulator-bridge`) stays, because it **is** the emulator: frame
pacing, input mapping, audio resampling, core selection and loading, rewind, and Metal
presentation. The `.ipa` is that engine plus a SwiftUI shell plus the libretro core dylibs.

One piece of apparently wasm-shaped code is **not** dead and must not be removed:
`cores::validate_wasm_module` and `CoreRegistry::attach_module`. They are the path the
built-in diagnostic stand-in core loads through, six tests depend on them, and the magic-header
check is a real guard. The name is a leftover; the code is live.

## The UI target

The deleted web UI is still the **design reference** for the SwiftUI app, through its
screenshots, which is why those were kept: `docs/library-mobile.png`, `docs/mobile-player.png`,
`docs/settings-mobile.png`, `docs/shot-detail.png`, `docs/detail-artwork.png`.

Library screen, as it should look on iOS:

- Dark, full-bleed, Netflix-style shell. Red accent (`Play`, active tab, logo tile), teal or
  cyan for metadata values.
- Top bar: logo, a search field reading `Search N titles...`, an import (`+`) button, and a
  small status indicator.
- A **featured hero** at the top: full-bleed cover art behind a `FEATURED` label, the game
  title at large weight, a metadata line (`NES · USA · 24.0 KB · Imported`), a short
  description, then a filled red `Play` pill and a grey `More info` pill.
- **Horizontal shelves** below it (`Recently added` and similar), each with a title and a
  count, holding cover-art cards. Each card carries a small system badge (`NES`, `SMS`) in
  its top-left corner.
- A **bottom tab bar**: Home, All Games, Favorites, Settings, with the active tab in red.
- A thin status strip above the tab bar for diagnostics.

Player screen:

- Back chevron, then a telemetry strip (fps, frames, audio latency in ms, core memory).
- The emulated surface, aspect-correct, on black.
- Touch controls overlaid: D-pad, A/B, L/R shoulders, SELECT/START, pause and volume, a
  `Save state` button, and controls for aspect, filter, speed and scale.
- Controls must not overlap each other or run off the edge of the screen. `docs/mobile-before.png`
  and `docs/mobile-after.png` show a layout bug of exactly that kind that was already fixed
  once on the web side; do not reintroduce it on iOS.

The current SwiftUI app is a black screen with a monospace debug HUD and a plain list. That
HUD is a diagnostic scaffold for bring-up, not the product UI. It should survive as something
toggleable or hidden behind Settings, because on a sideloaded build with no debugger it is the
only diagnostic there is, but it is not what the user should open the app into.

## Testing

**State this plainly rather than letting it be discovered: there is no automated test that
proves a core actually emulates correctly.** The browser smoke tests were that net and they
were deleted with the web app. Core correctness is currently verified only by a person playing
a game on a device, which is why `TESTING.md` exists.

The replacement, when it is written, must be native:

- Rust integration tests that `dlopen` a host-architecture core dylib through the native core
  loader and assert on real frames, audio and input. This needs a host build path in
  `scripts/build-core.sh` alongside the existing `ios` one.
- The existing `cargo test` suites (85 default, 95 with `--features native-core,uniffi-bindings`)
  cover pacing, audio, input, the registry, pixel conversion and the rewind tape. Keep them
  green, and keep both numbers accurate in documentation when they change.

## Systems

Shipping in the `.ipa` today, five cores, nine systems: NES (fceumm), SNES (snes9x),
GBA/GB/GBC (mgba), Genesis/Master System/Game Gear (genesis_plus_gx), PS1 (pcsx_rearmed).

Still ahead, hardest last: N64 (needs MoltenVK and the injected-Vulkan-context path), PSP, DS,
3DS, Switch. `native/switch-wrapper/` is a working inversion-of-control frame gate with no
engine behind it yet.

Architectural invariants (one MTLDevice owned by Rust and read back by Swift,
`engine.coreState` as the only source of truth for core residency, import-and-copy into
Documents with original filenames preserved, no security-scoped URLs on the launch path) are
documented in `SESSION_HANDOFF.md`. Do not relitigate them.
