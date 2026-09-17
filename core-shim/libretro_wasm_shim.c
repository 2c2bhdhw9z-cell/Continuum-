/*
 * libretro → WebAssembly host shim.
 *
 * A libretro core does not call the frontend directly: the frontend hands it
 * function *pointers* (`retro_set_video_refresh`, `retro_set_environment`, ...) and
 * the core calls those. In WebAssembly a host cannot manufacture a function pointer
 * inside another module's table, so a JS frontend has nothing valid to pass.
 *
 * This file closes that gap. It is compiled *into* the core module, so its
 * functions are real, in-table function pointers as far as the core is concerned.
 * Each one immediately forwards to an imported host function:
 *
 *     core (C)  ──calls fn ptr──▶  shim trampoline  ──wasm import──▶  host (JS)
 *
 * It also wraps the two entry points whose C struct layouts a host would otherwise
 * have to hard-code (`retro_game_info`, `retro_system_av_info`), flattening them
 * into plain arrays. That keeps struct offsets — which change between libretro API
 * revisions — on this side of the boundary, where the compiler checks them.
 *
 * Build: see scripts/build-core.sh. Nothing here is emulator-specific, so the same
 * shim serves fceumm, mgba, gambatte or any other libretro core.
 */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include <libretro.h>

#define HOST_IMPORT(sym) __attribute__((import_module("host"), import_name(sym)))
#define WASM_EXPORT(sym) __attribute__((export_name(sym)))

/* ------------------------------------------------------------------ imports */

/* `pitch` is narrowed to 32 bits: wasm32 size_t is 32 bits anyway, and keeping the
 * import signature to i32s avoids a BigInt marshalling cost per frame in JS. */
HOST_IMPORT("video_refresh")
extern void host_video_refresh(const void *data, unsigned width, unsigned height,
                               unsigned pitch);

/* Returns frames consumed, mirroring `retro_audio_sample_batch_t`. */
HOST_IMPORT("audio_batch")
extern unsigned host_audio_batch(const int16_t *data, unsigned frames);

HOST_IMPORT("input_poll")
extern void host_input_poll(void);

HOST_IMPORT("input_state")
extern int16_t host_input_state(unsigned port, unsigned device, unsigned index,
                               unsigned id);

/* Returns non-zero for "handled". `data` points into *this* module's memory, so the
 * host reads and writes it through a view of the core's linear memory. */
HOST_IMPORT("environment")
extern int host_environment(unsigned cmd, void *data);

/* ------------------------------------------------------------- trampolines */

static void shim_video_refresh(const void *data, unsigned width, unsigned height,
                               size_t pitch) {
   host_video_refresh(data, width, height, (unsigned)pitch);
}

static void shim_audio_sample(int16_t left, int16_t right) {
   /* Cores that emit sample-at-a-time are funnelled into the batch path so the host
    * only implements one. A two-sample frame is a cheap call, and no core in this
    * project's manifest uses this path in its hot loop. */
   const int16_t frame[2] = { left, right };
   host_audio_batch(frame, 1);
}

static size_t shim_audio_batch(const int16_t *data, size_t frames) {
   return (size_t)host_audio_batch(data, (unsigned)frames);
}

static void shim_input_poll(void) { host_input_poll(); }

static int16_t shim_input_state(unsigned port, unsigned device, unsigned index,
                                unsigned id) {
   return host_input_state(port, device, index, id);
}

static bool shim_environment(unsigned cmd, void *data) {
   return host_environment(cmd, data) != 0;
}

/* ---------------------------------------------------------------- exports */

/* Registers every callback. Must run before `retro_init`, because cores query the
 * environment callback during `retro_set_environment`. */
WASM_EXPORT("shim_install")
void shim_install(void) {
   retro_set_environment(shim_environment);
   retro_set_video_refresh(shim_video_refresh);
   retro_set_audio_sample(shim_audio_sample);
   retro_set_audio_sample_batch(shim_audio_batch);
   retro_set_input_poll(shim_input_poll);
   retro_set_input_state(shim_input_state);
}

/*
 * Flattens `retro_system_av_info` into 7 doubles:
 *   0 base_width  1 base_height  2 max_width  3 max_height
 *   4 aspect_ratio  5 fps  6 sample_rate
 *
 * Doubles throughout so the host reads one Float64Array and never worries about
 * mixed int/float/padding layout.
 */
WASM_EXPORT("shim_av_info")
void shim_av_info(double *out) {
   struct retro_system_av_info av;
   memset(&av, 0, sizeof(av));
   retro_get_system_av_info(&av);
   out[0] = (double)av.geometry.base_width;
   out[1] = (double)av.geometry.base_height;
   out[2] = (double)av.geometry.max_width;
   out[3] = (double)av.geometry.max_height;
   out[4] = (double)av.geometry.aspect_ratio;
   out[5] = av.timing.fps;
   out[6] = av.timing.sample_rate;
}

/*
 * Flattens `retro_system_info` into 5 u32s:
 *   0 library_name*  1 library_version*  2 valid_extensions*
 *   3 need_fullpath  4 block_extract
 *
 * The three pointers are NUL-terminated C strings in this module's memory.
 */
WASM_EXPORT("shim_system_info")
void shim_system_info(uint32_t *out) {
   struct retro_system_info info;
   memset(&info, 0, sizeof(info));
   retro_get_system_info(&info);
   out[0] = (uint32_t)(uintptr_t)info.library_name;
   out[1] = (uint32_t)(uintptr_t)info.library_version;
   out[2] = (uint32_t)(uintptr_t)info.valid_extensions;
   out[3] = info.need_fullpath ? 1u : 0u;
   out[4] = info.block_extract ? 1u : 0u;
}

/*
 * Loads content already copied into this module's memory.
 *
 * The host allocates with the exported `malloc`, writes the ROM there, then calls
 * this. The allocation must stay alive until `retro_unload_game`: cores are allowed
 * to keep `info.data` rather than copy it.
 */
WASM_EXPORT("shim_load_game")
int shim_load_game(const void *data, uint32_t size) {
   struct retro_game_info info;
   memset(&info, 0, sizeof(info));
   info.path = NULL; /* need_fullpath cores are rejected by the host loader. */
   info.data = data;
   info.size = (size_t)size;
   info.meta = NULL;
   return retro_load_game(&info) ? 1 : 0;
}

/* Exposes the shim's own contract version, so a host can refuse a stale core build. */
WASM_EXPORT("shim_abi_version")
uint32_t shim_abi_version(void) { return 1u; }
