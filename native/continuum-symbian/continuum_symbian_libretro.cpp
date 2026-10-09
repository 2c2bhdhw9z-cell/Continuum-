// Continuum Symbian — a libretro core this project is writing.
//
// The name is Continuum, not EKA2L1 and not libretro. The `_libretro` suffix is only the
// plug. The engine behind it is still the stub: load_game fails closed until EKA2L1 is
// linked. Nothing here draws a fake game.

#include "stub_engine.h"

#include <libretro.h>

#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <memory>

namespace {

retro_environment_t environ_cb = nullptr;
retro_log_printf_t log_cb = nullptr;
std::unique_ptr<continuum::StubEngine> g_engine;

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

RETRO_API void retro_set_video_refresh(retro_video_refresh_t) {}
RETRO_API void retro_set_audio_sample(retro_audio_sample_t) {}
RETRO_API void retro_set_audio_sample_batch(retro_audio_sample_batch_t) {}
RETRO_API void retro_set_input_poll(retro_input_poll_t) {}
RETRO_API void retro_set_input_state(retro_input_state_t) {}

RETRO_API void retro_init(void) { g_engine = std::make_unique<continuum::StubEngine>(); }

RETRO_API void retro_deinit(void) {
  if (g_engine != nullptr) g_engine->Shutdown();
  g_engine.reset();
}

RETRO_API void retro_set_controller_port_device(unsigned, unsigned) {}

RETRO_API bool retro_load_game(const struct retro_game_info*) {
  if (g_engine == nullptr || !g_engine->EngineLinked()) {
    Log("%s", g_engine != nullptr ? g_engine->Refusal()
                                  : "Continuum Symbian is not initialised.");
    return false;
  }
  return false;
}

RETRO_API bool retro_load_game_special(unsigned, const struct retro_game_info*, size_t) {
  return false;
}

RETRO_API void retro_unload_game(void) {}
RETRO_API unsigned retro_get_region(void) { return RETRO_REGION_NTSC; }
RETRO_API void retro_reset(void) {}
RETRO_API void retro_run(void) {}

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
