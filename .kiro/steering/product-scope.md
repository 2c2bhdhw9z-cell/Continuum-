# Product scope: the .ipa is the only deliverable

## There will not be a PWA

Continuum ships as **one artefact: a sideloadable iOS `.ipa`**. It is an all-in-one emulator
for iPhone, installed through TrollStore, with JIT and the increased-memory entitlements.

The web app under `web/` was scaffolding. Its job was to prove the Rust engine worked before
there was any way to compile for iOS, and that job is finished. It is **not** a deliverable,
it is **not** a second supported target, and the project is **not** dual-platform.

Rules that follow from this, and they are not negotiable:

- Do not add features to `web/`. Do not polish it, do not fix its cosmetics, do not extend it.
- Do not describe the project as having a web half and an iOS half. It has one product.
- `web/` and `.github/workflows/deploy.yml` are slated for removal. Treat them as dead code
  that has not been deleted yet, not as something to maintain.
- Do not keep `web/` alive on the grounds that the browser tests live there. If a test is
  worth having, write it natively (Rust integration test, or a Swift/XCTest target). See
  "Testing" below.
- "Done" for this project means: the `.ipa` plays every supported system, and the only
  remaining work is steady updates for users of that `.ipa`.

## What stays

The Rust engine (`crates/emulator-bridge`) stays, because it **is** the emulator: frame
pacing, input mapping, audio resampling, core selection and loading, and Metal presentation.
The `.ipa` is that engine plus a SwiftUI shell plus the libretro core dylibs.

The engine's wasm support (`src/wasm.rs`, the `MaybeSend` split, the wasm-only dependencies)
is feature-gated and costs the iOS build nothing. Removing it is optional cleanup, not a
priority, and must never be done in a way that risks the native path.

## The UI target: make the .ipa look like the PWA

The web UI is the **design reference** for the SwiftUI app. This is the one thing the PWA is
still good for. Screenshots in `docs/`: `library-mobile.png`, `mobile-player.png`,
`settings-mobile.png`, `shot-detail.png`, `detail-artwork.png`.

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

The `.ipa` has no automated test that proves a core actually emulates correctly. The browser
smoke tests were that net, and they are going away with `web/`.

Replacements must be native:

- Rust integration tests that `dlopen` a host-architecture core dylib through
  `NativeLibretroCore` and assert on real frames, audio and input. This needs a host build
  path in `scripts/build-core.sh` alongside the existing `ios` one.
- The existing `cargo test` suites (74 default, 84 with `--features native-core,uniffi-bindings`)
  already cover pacing, audio, input, the registry and pixel conversion. Keep them green.

Do not delete the browser tests without saying plainly, in the same change, that core
behaviour is then untested until a native replacement lands.

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
