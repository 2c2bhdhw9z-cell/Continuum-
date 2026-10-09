// Continuum Symbian — a libretro core this project is writing.
//
// The name is Continuum, not EKA2L1 and not libretro. The `_libretro` suffix is only the
// plug. On the phone (CONTINUUM_SYMBIAN_EKA2L1, set by scripts/build-core.sh) the engine is
// EKA2L1's jitless iOS port. On the host it is the stub, and load_game fails closed. Nothing
// here draws a fake game.

#if defined(CONTINUUM_SYMBIAN_EKA2L1)
#include "eka2l1_engine.h"
using EngineImpl = continuum::Eka2l1Engine;
#else
#include "stub_engine.h"
using EngineImpl = continuum::StubEngine;
#endif

#include <string>
#include <vector>

#include <libretro.h>

#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <cstdint>
#include <memory>

namespace {

retro_environment_t environ_cb = nullptr;
retro_log_printf_t log_cb = nullptr;
std::unique_ptr<EngineImpl> g_engine;
retro_video_refresh_t video_cb = nullptr;
retro_input_poll_t poll_cb = nullptr;
retro_input_state_t input_cb = nullptr;
retro_audio_sample_batch_t audio_batch_cb = nullptr;
const char* system_dir = nullptr;
bool g_loaded = false;
bool g_touching = false;
std::vector<std::uint32_t> g_video;
int g_video_w = 0;
int g_video_h = 0;

void Log(const char* format, ...) {
  char buffer[512];
  va_list args;
  va_start(args, format);
  std::vsnprintf(buffer, sizeof(buffer), format, args);
  va_end(args);
  if (log_cb != nullptr) {
    log_cb(RETRO_LOG_ERROR, "%s\n", buffer);
  } else {
    std::fprintf(stderr, "[Continuum Symbian] %s\n", buffer);
  }
}

}  // namespace

extern "C" {

RETRO_API unsigned retro_api_version(void) { return RETRO_API_VERSION; }

RETRO_API void retro_get_system_info(struct retro_system_info* info) {
  std::memset(info, 0, sizeof(*info));
  info->library_name = "Continuum Symbian";
  info->library_version = "0.0.1";
  info->need_fullpath = true;
  info->valid_extensions = "sis|sisx|n-gage";
  info->block_extract = true;
}

RETRO_API void retro_get_system_av_info(struct retro_system_av_info* info) {
  continuum::ScreenInfo screen =
      g_engine != nullptr ? g_engine->GetScreenInfo() : continuum::ScreenInfo{};
  std::memset(info, 0, sizeof(*info));
  info->geometry.base_width = screen.width;
  info->geometry.base_height = screen.height;
  info->geometry.max_width = screen.width;
  info->geometry.max_height = screen.height;
  info->geometry.aspect_ratio = screen.aspect_ratio;
  info->timing.fps = screen.refresh_rate;
  info->timing.sample_rate = screen.sample_rate;
}

RETRO_API void retro_set_environment(retro_environment_t cb) {
  environ_cb = cb;
  bool no_game = false;
  cb(RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME, &no_game);
  retro_log_callback logging{};
  if (cb(RETRO_ENVIRONMENT_GET_LOG_INTERFACE, &logging)) log_cb = logging.log;
}

RETRO_API void retro_set_video_refresh(retro_video_refresh_t cb) { video_cb = cb; }
RETRO_API void retro_set_audio_sample(retro_audio_sample_t) {}
RETRO_API void retro_set_audio_sample_batch(retro_audio_sample_batch_t cb) { audio_batch_cb = cb; }
RETRO_API void retro_set_input_poll(retro_input_poll_t cb) { poll_cb = cb; }
RETRO_API void retro_set_input_state(retro_input_state_t cb) { input_cb = cb; }

RETRO_API void retro_init(void) { g_engine = std::make_unique<EngineImpl>(); }

RETRO_API void retro_deinit(void) {
  if (g_engine != nullptr) g_engine->Shutdown();
  g_engine.reset();
}

RETRO_API void retro_set_controller_port_device(unsigned, unsigned) {}

RETRO_API bool retro_load_game(const struct retro_game_info* game) {
  if (g_engine == nullptr || !g_engine->EngineLinked()) {
    Log("%s", g_engine != nullptr ? g_engine->Refusal()
                                  : "Continuum Symbian is not initialised.");
    return false;
  }
  if (game == nullptr || game->path == nullptr) {
    Log("Continuum Symbian needs a .sis, .sisx or .n-gage file.");
    return false;
  }
  if (environ_cb != nullptr) {
    environ_cb(RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY, &system_dir);
    retro_pixel_format format = RETRO_PIXEL_FORMAT_XRGB8888;
    environ_cb(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, &format);
  }
  continuum::EngineConfig config;
  config.system_dir = system_dir;
  if (!g_engine->Initialise(config)) {
    Log("%s", g_engine->Refusal());
    return false;
  }
  const std::string firmware = std::string(system_dir) + "/continuum-symbian/firmware";
  if (!g_engine->InstallFirmware(firmware.c_str()) || !g_engine->InstallPackage(game->path) ||
      !g_engine->Boot()) {
    Log("%s", g_engine->Refusal());
    g_engine->Shutdown();
    return false;
  }
  g_loaded = true;
  return true;
}

RETRO_API bool retro_load_game_special(unsigned, const struct retro_game_info*, size_t) {
  return false;
}

RETRO_API void retro_unload_game(void) {
  if (g_engine != nullptr && g_loaded) g_engine->Shutdown();
  g_loaded = false;
}
RETRO_API unsigned retro_get_region(void) { return RETRO_REGION_NTSC; }
RETRO_API void retro_reset(void) {}
RETRO_API void retro_run(void) {
  if (!g_loaded || g_engine == nullptr) return;
  if (poll_cb != nullptr) poll_cb();
  if (input_cb != nullptr) {
    std::uint32_t keys = 0;
    for (unsigned id = 0; id <= RETRO_DEVICE_ID_JOYPAD_R3; ++id) {
      if (input_cb(0, RETRO_DEVICE_JOYPAD, 0, id)) keys |= 1u << id;
    }
    g_engine->SetKeys(keys);
#if defined(CONTINUUM_SYMBIAN_EKA2L1)
    // The pointer is -0x7fff..0x7fff over the picture; the guest screen is 240x320.
    const bool pressed = input_cb(0, RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_PRESSED) != 0;
    if (pressed || g_touching) {
      const int px = input_cb(0, RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_X);
      const int py = input_cb(0, RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_Y);
      const continuum::ScreenInfo screen = g_engine->GetScreenInfo();
      const int x = static_cast<int>((px + 0x7fff) * static_cast<long>(screen.width) / 0xfffe);
      const int y = static_cast<int>((py + 0x7fff) * static_cast<long>(screen.height) / 0xfffe);
      g_engine->Touch(x, y, pressed ? (g_touching ? 1 : 0) : 2);
      g_touching = pressed;
    }
#endif
  }
  g_engine->RunFrame();
#if defined(CONTINUUM_SYMBIAN_EKA2L1)
  if (g_engine->LatestFrame(g_video, g_video_w, g_video_h) && video_cb != nullptr) {
    video_cb(g_video.data(), g_video_w, g_video_h, static_cast<size_t>(g_video_w) * 4);
  } else if (video_cb != nullptr) {
    video_cb(nullptr, 0, 0, 0);
  }
#endif
  // The emulator plays its own sound (cubeb, straight to CoreAudio). Silence keeps the host's
  // audio clock moving.
  if (audio_batch_cb != nullptr) {
    static std::int16_t silence[800 * 2] = {};
    audio_batch_cb(silence, 800);
  }
}

RETRO_API size_t retro_serialize_size(void) { return 0; }
RETRO_API bool retro_serialize(void*, size_t) { return false; }
RETRO_API bool retro_unserialize(const void*, size_t) { return false; }

RETRO_API void retro_cheat_reset(void) {}
RETRO_API void retro_cheat_set(unsigned, bool, const char*) {}

RETRO_API void* retro_get_memory_data(unsigned) { return nullptr; }
RETRO_API size_t retro_get_memory_size(unsigned) { return 0; }

RETRO_API int continuum_symbian_engine_linked(void) {
  return g_engine != nullptr && g_engine->EngineLinked() ? 1 : 0;
}

}  // extern "C"
