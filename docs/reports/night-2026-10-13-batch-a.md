# Night batch A, 13 October 2026: what landed, and what was handed on

Batch A is the small leftovers of the 12 October cleanup (`docs/reports/cleanup-2026-10-12/`) plus
the licence-swap leftovers from STATUS.md "Next up" item 5. Two other agents worked on the same
repository the same night, so anything that could only be fixed inside `cores/continuum-gb/` or
`crates/emulator-bridge/src/peripherals/` is written down here instead of changed.

## Handed on (another agent owns the file)

- **RUST-19, the Amiibo tag id in the tap line.** The hex id is built in
  `crates/emulator-bridge/src/peripherals/amiibo.rs` (`tap`, both the "tapped" and the "not
  tapped" line). It should be dropped: the player tapped a figure they can already see by name,
  and the id means nothing to them. Keep the id in the activity log if it is wanted for support.
- **IOS-12, deciding an Amiibo tap from text.** The same `tap` function returns one sentence and
  the app decides success with `line.contains("tapped")`. The clean fix is a real result from the
  engine rather than a sentence to match.

## Notes for whoever builds next

- **Editing `scripts/build-core.sh` rebuilds every core** (the per-core fingerprint hashes the
  whole script apart from its `IOS_PIN=` lines), so the next build after one pays the long
  45-50 minutes instead of 6-8. Editing one `scripts/patches/<family>-*.patch` rebuilds that core
  only.
