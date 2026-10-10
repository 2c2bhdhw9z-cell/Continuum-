# Review coverage

A file-by-file read of the repository, area by area. "Read" means every line was read, not
searched. Vendored third-party core sources are out of scope; our patches to them are in.

## Area 1: unread patches and Continuum Symbian (9 October 2026)

| File | Read | Findings |
| --- | --- | --- |
| scripts/patches/azahar-libretro-camera-and-amiibo.patch | Y | Camera: bounds-checked, fine. Amiibo poll opens the stamp file on every frame (~60 small file reads a second while a 3DS game runs). Minor cost; left, because editing a patch's new-file hunk risks the build. Logged for later. |
| scripts/patches/azahar-do-not-wait-on-pipeline-compile.patch | Y | Works as intended. Side effect, documented in the patch: draws whose pipeline is still compiling are skipped (brief pop-in). The software-vertex path also drops that batch. No bug. |
| scripts/patches/continuum-symbian-frame-readback.patch | Y | Fine. Binds the framebuffer before the read-back hook; the present right after rebinds the renderbuffer. |
| scripts/patches/parallel_n64-aarch64-gate-hot-state-on-new-dynarec.patch | Y | Fine; the fix and its reason are correct. |
| scripts/patches/parallel_n64-reapply-variables-after-initiate-gfx.patch | Y | Fine. The forward declaration only works because `update_variables` is not `static` upstream; a pin move that makes it static would fail to compile (loudly). |
| native/continuum-symbian/continuum_symbian_libretro.cpp | Y | FIXED: a touch on the far edge mapped to x = 240 / y = 320, one past the screen; now clamped. Noted: video is sent at whatever size the EKA2L1 backbuffer is, while max geometry is fixed at 240x320; a landscape or larger backbuffer would exceed it (unknown until a phone runs it). No save states (returns 0), as documented. |
| native/continuum-symbian/eka2l1_engine.mm | Y | FIXED: the last game's frame survived Shutdown, so the next Symbian game could open on the old picture; now cleared. Noted: Boot returns success even when the package install fails (on purpose, shows the phone menu), but the refusal text is then never shown to the user. |
| native/continuum-symbian/eka2l1_engine.h | Y | None. |
| native/continuum-symbian/symbian_engine.h | Y | None. |
| native/continuum-symbian/stub_engine.h / .cpp | Y | None. |
| native/continuum-symbian/test_harness.cpp | Y | None. Could not run here: the box has no C++ compiler (no g++/clang++). |
| native/continuum-symbian/build.sh | Y | None. |
