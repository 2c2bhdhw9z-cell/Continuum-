# Cleanup batch, pass 2: Amiibo cleanup on every launch, and the fix phase re-checked

This pass reviews iteration 2 of the fix phase (`a10ffab`) and re-checks the whole phase (`15fd146..a10ffab`) against the nine K items and the 20 CONFIRMED HIGH/MEDIUM findings. The only blocking item from pass 1, RUST-03 (an old Amiibo tapped by itself after the app was swiped away or closed by iOS), is fixed: `launch(entry:)` now calls `peripherals.endAmiiboSession()` on every launch, before the core loads, and the misleading comment in `stopSession()` is corrected. All nine K items and all 20 findings are done, and VERIFICATION.md has evidence for every push, run p8 included. The five non-blocking items from pass 1 are still open, this pass adds two small ones, and none of the seven blocks.

Watch for: a failed restore still drops its "older copies" line and sticks it onto the next restore's finished line (**confirmed**, carried from pass 1), and a clock game's `<game>.rtc` is still rewritten on every 20-second flush (**confirmed**, carried). The retired address is still in four commits of git history, and only the owner can approve the force push needed to remove it (**confirmed**).

**Verdict**: APPROVED

## High-level view

The Amiibo hand-over files (`continuum-amiibo.bin`, `.stamp`, `.source`) are now cleared at the start of every launch, not only when a session ends inside the app, so the cold start after a swipe-away no longer hands the last tag to the next 3DS game. The call writes back what the game saved to the figure before deleting, so the RUST-04 fix still holds on this path. `launch(entry:)` is the only caller of `engine.launch`, so no other way into a core skips it.

Every K item is done at the lines it names: address, QA Wolf, licence, clippy in CI, cBoy out of the cache key with its own push trigger, intro code gone, `cboy_renderer` defaulting to Standard (and surviving state loads and run-ahead through `apply_live`), Mealybug pinned, and the comments fixed. The boot ROM text that testers read now ends "Without the file the game simply starts", and no option text promises a startup picture. `boot_logo` in `gb.rs` remains, but it only fills tile data the way the real boot leaves it and writes no tile map, so nothing is shown. The only `fifo.rs` changes are the removed debug hooks, so the parked pixel work was not started.

No behaviour the owner proved on the phone was changed. Standard is the per-line drawer that played Yellow on build 177. A restore onto an empty install, or from an old backup with no dates, still lets the backup win as before. The feedback form is always open again, as on build 126. NES falls back to FCEUmm only when Mesen is missing from the build, never over a stored choice. Nothing under "Already decided by the owner" was reopened.

Every commit from `6193625` to the current head has `[skip ci]`. No file was added, so no ROMs. `.git/config` was last written before the fix phase, and only `master` exists, locally and on the remote, with one worktree. Two commits landed during this review, `a6befb9` and `54e5bd6`. Both change only owner-rules (the decisions to swap out the non-commercial cores and to drop arcade for now) and have `[skip ci]`. VERIFICATION.md records every HANDOFF check for all eight runs, plus the bindings check for the one push that changed the exported API (`f14dc62`).

<details>
<summary>Issues (7)</summary>

1. **Older copies line lost after a failed merge** (confirmed, non-blocking): when the restore merge returns `ok == false`, the copies it already moved to Files, Continuum, Older copies are never mentioned, and the stored line is added to the next successful restore's message. On failure, add the line to the failure status and set `olderCopiesLine = nil`.
2. **`<game>.rtc` rewritten on every flush** (confirmed, non-blocking): cBoy stamps bytes 16-24 of the clock block with `now_secs()` every frame, so `write_save_file`'s "unchanged" check never matches. Compare bytes 0-16 and skip the write when only the stamp moved.
3. **Generic refusal for other cores** (confirmed, non-blocking): cores this project didn't write still show "retro_load_game rejected the content" when they give no reason. Add RUST-02's casual fallback ("This game didn't start on <core>") together with RUST-08/RUST-09.
4. **Startup-picture notes and drawer order in the docs** (confirmed, non-blocking): STATUS.md lines 21, 95 and 1260 still describe or plan a cBoy startup picture, and neither STATUS.md "Next up" nor HANDOFF.md lists the exact drawer last. Fix both in the docs pass.
5. **Old address in git history** (confirmed, non-blocking): four commits (`20bd9cb`, `016555b`, `fd51c47`, `97c0367`) still contain the retired address, and owner-rules says "history included". This needs a history rewrite and a force push, so ask the owner first.
6. **BIOS doc comment on the Symbian function** (confirmed, non-blocking): `installSymbianFirmwareFromDocuments` was put between `installBiosFromDocuments` and its doc comment, so the BIOS explanation now sits on the Symbian function and `installBiosFromDocuments` has none. Move the Symbian function, with its own comment, above or below the BIOS comment block.
7. **No phone test for the cBoy clock or tilt fixes** (confirmed, non-blocking): TESTING.md section K covers the backup, restore, auto-save, feedback-form and drawer fixes, but not the cartridge clock (Pokémon Gold/Silver/Crystal keeping time while closed) or phone motion (a tilt cartridge). Add one plain row each in the docs pass.

</details>

<details>
<summary>Details</summary>

### Restore failure path and clock churn (carried from pass 1)

```swift
// CloudSync.swift:1303-1313, the restore's afterSync
self?.olderCopiesLine = Self.moveSetAsideCopies(
    from: root.appendingPathComponent("Conflicts", isDirectory: true), except: alreadyAside)
guard ok else { return }          // line never shown, never cleared
self?.finishRestore(backupSettings: backupSettings)
```

When the merge stops part-way, the copies it already set aside are moved to Older copies, but the screen keeps only "sync folder failed: …". The stored line then waits until the next successful restore, and `finishRestore` (`CloudSync.swift:1380`) adds it there with the old run's count (**confirmed**). Nothing is lost, but the player is told about the copies at the wrong time.

`write_rtc_block` (`cores/continuum-gb/src/libretro.rs:215`) writes the wall-clock stamp into bytes 16-24 on every frame. `write_save_file` (`crates/emulator-bridge/src/bridge.rs:1851-1856`) skips a write only when the whole file matches. So every periodic flush of a clock game rewrites `<game>.rtc`, and every sync uploads it again (**confirmed**). It is only 24 bytes, but it is the mtime churn that check exists to prevent, and on a live folder shared by two devices the file always looks changed.

### Docs and comments left behind

`installSymbianFirmwareFromDocuments` (`ContinuumApp.swift:3398`) was inserted under the last line of `installBiosFromDocuments`'s doc comment. Swift attaches the whole block to the Symbian function, including the "THIS EXISTS BECAUSE THE TWO DIRECTORIES ARE NOT THE SAME ONE" explanation, and `installBiosFromDocuments` at `:3447` is left with no doc comment (**confirmed**). It is a comment-only problem.

TESTING.md's new section K has five rows. None asks the owner to check that a Pokémon Gold/Silver/Crystal clock keeps time after the game is closed (CBOY-01/03/09), or that a tilt cartridge reacts to the phone (CBOY-02) (**confirmed**). owner-rules asks for TESTING.md to be updated in the same push as any change the owner needs to try. cBoy tests cover the core side, but only a phone can show whether the app really keeps the `.rtc` and starts CoreMotion.

</details>

<details>
<summary>File map</summary>

Iteration 2 (`a10ffab`):
- `native/ios/ContinuumApp.swift`: `endAmiiboSession()` on every launch, before the core loads; `stopSession()` comment corrected.
- `native/ios/Peripherals.swift`: `endAmiiboSession()` doc comment.

Whole fix phase (`15fd146..a10ffab`):
- `.github/workflows/ios.yml`: Clippy step; cBoy push trigger; cBoy out of `CORE_SOURCES_HASH`.
- `.github/workflows/qawolf-mobile.yml`, `.kiro/settings/mcp.json`: deleted (QA Wolf).
- `Cargo.toml`, `cores/continuum-gb/Cargo.toml`: GPL-3.0-only.
- `native/ios/build-engine.sh`, `scripts/build-core.sh`: drop the cached cBoy dylib; no rebuilt marker for unpinned cores.
- `cores/continuum-gb/scripts/fetch-test-roms.sh`, `tests/roms.rs`, `tests/options_e2e.rs`: pinned Mealybug with tarball fallback; scroll test skips; both drawers tested.
- `cores/continuum-gb/src/` (`options.rs`, `ppu.rs`, `fifo.rs`, `bus.rs`, `gb.rs`, `libretro.rs`, `cart.rs`, `lib.rs`, `gba/bus.rs`): drawer option; intro removed; boot/BIOS/palette text; sensor id; RTC block and catch-up; comments.
- `crates/emulator-bridge/src/` (`bridge.rs`, `cores/native_core.rs`, `cores/diagnostic.rs`, `sync/mod.rs`, `uniffi_online.rs`): `<game>.rtc`; `unsafe extern "C"` log forward; `refusal_from_log`; `mirror_plan` / `RemoveRemote`.
- `native/ios/` (`BackupArchive.swift`, `CloudSync.swift`, `SaveStates.swift`, `ContinuumApp.swift`, `Peripherals.swift`, `Feedback.swift`, `SettingsScreen.swift`): backup/restore fixes, refused auto-save to a slot, NES fallback, Symbian firmware copy, cBoy firmware into `cboy/`, Amiibo write-back and cleanup, new address, unfoldable forms.
- `README.md`, `STATUS.md`, `HANDOFF.md`, `TESTING.md`, `docs/MANIC_PARITY.md`, `docs/REVIEW_COVERAGE.md`: address, QA Wolf removal, cache notes, TESTING K1-K5 and O1.

Full diff: `git -C /projects/sandbox/Continuum- diff 15fd146 a10ffab` (iteration 2 alone: `git show a10ffab`).

</details>
