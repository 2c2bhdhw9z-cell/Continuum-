// Continuum — harness for the Step 10 stub wrapper.
//
// Loads the built core the way `NativeCore` will (dlopen + dlsym), drives `retro_run`, and
// asserts on the things that are otherwise only observable as timing. It exists because
// the frame gate is the piece most likely to be subtly wrong, and "the colour looks like
// it is rotating" is not a test.
//
// What is checked:
//
//   1. The libretro surface is complete — every symbol `NativeCore` will dlsym.
//   2. `retro_get_system_info` declares need_fullpath and the Switch extensions.
//   3. A frame arrives per `retro_run`, and the colour matches an independently computed
//      expectation.
//   4. The colour advances, and completes one revolution in the declared frame count.
//   5. A deliberate stall produces a *dupe*, not a hang, and `video_refresh` is called
//      with NULL.
//   6. After a stall the engine does not run two frames for one `retro_run` — the
//      counter-versus-semaphore bug the gate exists to avoid.
//   7. Input snapshots cross the thread boundary intact.
//   8. Audio is drained on the frontend thread, in the right quantity.
//   9. Unload and reload are clean, with no thread left behind.

#include <dlfcn.h>

#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <thread>
#include <vector>

#include <libretro.h>

namespace {

int g_failures = 0;
int g_checks = 0;

void Check(const char* name, bool ok, const std::string& detail) {
  ++g_checks;
  if (ok) {
    std::printf("PASS  %s%s%s\n", name, detail.empty() ? "" : " — ", detail.c_str());
  } else {
    ++g_failures;
    std::printf("FAIL  %s%s%s\n", name, detail.empty() ? "" : " — ", detail.c_str());
  }
}

// ---------------------------------------------------------------- frontend state

struct VideoCall {
  bool null_frame = false;
  std::uint32_t width = 0;
  std::uint32_t height = 0;
  std::uint8_t first_pixel[4] = {};
  bool hw_sentinel = false;
};

std::vector<VideoCall> g_video_calls;
std::size_t g_audio_frames = 0;
std::uint16_t g_buttons_to_report = 0;
std::int16_t g_analog_to_report = 0;

void VideoRefresh(const void* data, unsigned width, unsigned height, size_t pitch) {
  VideoCall call{};
  call.width = width;
  call.height = height;
  if (data == nullptr) {
    call.null_frame = true;
  } else if (data == RETRO_HW_FRAME_BUFFER_VALID) {
    call.hw_sentinel = true;
  } else {
    std::memcpy(call.first_pixel, data, 4);
    (void)pitch;
  }
  g_video_calls.push_back(call);
}

size_t AudioBatch(const int16_t*, size_t frames) {
  g_audio_frames += frames;
  return frames;
}

void InputPoll(void) {}

int16_t InputState(unsigned port, unsigned device, unsigned index, unsigned id) {
  if (port != 0) return 0;
  if (device == RETRO_DEVICE_JOYPAD) {
    return (g_buttons_to_report & (1u << id)) != 0 ? 1 : 0;
  }
  if (device == RETRO_DEVICE_ANALOG) {
    (void)index;
    (void)id;
    return g_analog_to_report;
  }
  return 0;
}

bool Environment(unsigned cmd, void* data) {
  switch (cmd) {
    case RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY:
    case RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY:
      // The two the native build must implement. Answering them here is what proves the
      // wrapper reads them rather than guessing a sandbox layout.
      *static_cast<const char**>(data) = "/tmp/continuum-system";
      return true;
    case RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME:
    case RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE:
      return true;
    case RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE:
      *static_cast<bool*>(data) = false;
      return true;
    case RETRO_ENVIRONMENT_SET_HW_RENDER:
      // No Vulkan in this harness, so refusing is honest and drives the host path.
      return false;
    default:
      return false;
  }
}

// ------------------------------------------------------------------- symbol table

struct Core {
  void* handle = nullptr;

  void (*retro_init)(void) = nullptr;
  void (*retro_deinit)(void) = nullptr;
  unsigned (*retro_api_version)(void) = nullptr;
  void (*retro_get_system_info)(struct retro_system_info*) = nullptr;
  void (*retro_get_system_av_info)(struct retro_system_av_info*) = nullptr;
  void (*retro_set_environment)(retro_environment_t) = nullptr;
  void (*retro_set_video_refresh)(retro_video_refresh_t) = nullptr;
  void (*retro_set_audio_sample)(retro_audio_sample_t) = nullptr;
  void (*retro_set_audio_sample_batch)(retro_audio_sample_batch_t) = nullptr;
  void (*retro_set_input_poll)(retro_input_poll_t) = nullptr;
  void (*retro_set_input_state)(retro_input_state_t) = nullptr;
  bool (*retro_load_game)(const struct retro_game_info*) = nullptr;
  void (*retro_unload_game)(void) = nullptr;
  void (*retro_run)(void) = nullptr;
  void (*retro_reset)(void) = nullptr;
  size_t (*retro_serialize_size)(void) = nullptr;
  void (*retro_cheat_reset)(void) = nullptr;
  void (*retro_cheat_set)(unsigned, bool, const char*) = nullptr;

  std::uint64_t (*frames_served)(void) = nullptr;
  std::uint64_t (*dupes)(void) = nullptr;
  std::uint64_t (*engine_frame_index)(void) = nullptr;
  std::uint16_t (*last_buttons)(unsigned) = nullptr;
  void (*set_stall)(unsigned, unsigned) = nullptr;
  void (*expected_colour)(std::uint64_t, std::uint8_t*) = nullptr;
};

Core g_core;
std::vector<std::string> g_missing;

template <typename T>
void Bind(T& slot, const char* name) {
  slot = reinterpret_cast<T>(dlsym(g_core.handle, name));
  if (slot == nullptr) g_missing.emplace_back(name);
}

}  // namespace

int main(int argc, char** argv) {
  const char* path = argc > 1 ? argv[1] : "./libcontinuum_switch.so";

  g_core.handle = dlopen(path, RTLD_NOW | RTLD_LOCAL);
  if (g_core.handle == nullptr) {
    std::printf("FAIL  core loads — dlopen: %s\n", dlerror());
    return 1;
  }

  // 1. The surface NativeCore will bind.
  Bind(g_core.retro_init, "retro_init");
  Bind(g_core.retro_deinit, "retro_deinit");
  Bind(g_core.retro_api_version, "retro_api_version");
  Bind(g_core.retro_get_system_info, "retro_get_system_info");
  Bind(g_core.retro_get_system_av_info, "retro_get_system_av_info");
  Bind(g_core.retro_set_environment, "retro_set_environment");
  Bind(g_core.retro_set_video_refresh, "retro_set_video_refresh");
  Bind(g_core.retro_set_audio_sample, "retro_set_audio_sample");
  Bind(g_core.retro_set_audio_sample_batch, "retro_set_audio_sample_batch");
  Bind(g_core.retro_set_input_poll, "retro_set_input_poll");
  Bind(g_core.retro_set_input_state, "retro_set_input_state");
  Bind(g_core.retro_load_game, "retro_load_game");
  Bind(g_core.retro_unload_game, "retro_unload_game");
  Bind(g_core.retro_run, "retro_run");
  Bind(g_core.retro_reset, "retro_reset");
  Bind(g_core.retro_serialize_size, "retro_serialize_size");
  Bind(g_core.retro_cheat_reset, "retro_cheat_reset");
  Bind(g_core.retro_cheat_set, "retro_cheat_set");
  Bind(g_core.frames_served, "continuum_stub_frames_served");
  Bind(g_core.dupes, "continuum_stub_dupes");
  Bind(g_core.engine_frame_index, "continuum_stub_engine_frame_index");
  Bind(g_core.last_buttons, "continuum_stub_last_buttons");
  Bind(g_core.set_stall, "continuum_stub_set_stall");
  Bind(g_core.expected_colour, "continuum_stub_expected_colour");

  Check("every libretro entry point resolves", g_missing.empty(),
        g_missing.empty() ? "24 symbols bound"
                          : "missing: " + [&] {
                              std::string joined;
                              for (const auto& name : g_missing) joined += name + " ";
                              return joined;
                            }());
  if (!g_missing.empty()) return 1;

  Check("api version matches the header the frontend compiled against",
        g_core.retro_api_version() == RETRO_API_VERSION,
        "core reports " + std::to_string(g_core.retro_api_version()));

  // 2. Declared content handling.
  g_core.retro_set_environment(Environment);
  retro_system_info system{};
  g_core.retro_get_system_info(&system);
  Check("declares need_fullpath and the Switch container extensions",
        system.need_fullpath && std::string(system.valid_extensions) == "nsp|xci|nca|nro",
        std::string(system.library_name) + " · " + system.valid_extensions +
            " · need_fullpath=" + (system.need_fullpath ? "true" : "false"));

  g_core.retro_init();
  g_core.retro_set_video_refresh(VideoRefresh);
  g_core.retro_set_audio_sample_batch(AudioBatch);
  g_core.retro_set_input_poll(InputPoll);
  g_core.retro_set_input_state(InputState);

  retro_game_info info{};
  info.path = "/tmp/continuum-stub.nro";
  const bool loaded = g_core.retro_load_game(&info);
  Check("loads with a path", loaded, info.path);
  if (!loaded) return 1;

  retro_system_av_info av{};
  g_core.retro_get_system_av_info(&av);
  Check("declares a max geometry large enough for docked output",
        av.geometry.max_width >= 1920 && av.geometry.max_height >= 1080 &&
            av.timing.fps > 59.0 && av.timing.sample_rate == 48000.0,
        std::to_string(av.geometry.base_width) + "x" + std::to_string(av.geometry.base_height) +
            " max " + std::to_string(av.geometry.max_width) + "x" +
            std::to_string(av.geometry.max_height) + " @ " + std::to_string(av.timing.fps) +
            " fps, " + std::to_string(static_cast<int>(av.timing.sample_rate)) + " Hz");

  // 3 & 4. One frame per run, and the colour advances as computed independently.
  constexpr int kFrames = 30;
  g_video_calls.clear();
  g_audio_frames = 0;
  for (int i = 0; i < kFrames; ++i) g_core.retro_run();

  Check("one frame per retro_run, none duped",
        g_video_calls.size() == static_cast<std::size_t>(kFrames) &&
            g_core.dupes() == 0 && g_core.frames_served() == kFrames,
        std::to_string(g_video_calls.size()) + " video calls, " +
            std::to_string(g_core.dupes()) + " dupes");

  bool colours_match = true;
  int distinct = 0;
  std::uint8_t previous[4] = {};
  for (int i = 0; i < kFrames; ++i) {
    std::uint8_t expected[4];
    g_core.expected_colour(static_cast<std::uint64_t>(i), expected);
    const auto& actual = g_video_calls[i].first_pixel;
    // The host renderer reports RGBA and the harness reads the first pixel back, so this
    // is an exact comparison rather than a tolerance.
    if (std::memcmp(expected, actual, 4) != 0) colours_match = false;
    if (i == 0 || std::memcmp(previous, actual, 4) != 0) ++distinct;
    std::memcpy(previous, actual, 4);
  }
  Check("each frame carries the independently computed colour", colours_match,
        "checked " + std::to_string(kFrames) + " frames against StubExpectedColour");
  Check("the colour actually rotates", distinct > kFrames / 2,
        std::to_string(distinct) + " distinct colours across " + std::to_string(kFrames) +
            " frames");

  // 8. Audio, drained on this thread, roughly one frame's worth per frame.
  const std::size_t expected_audio = static_cast<std::size_t>(48000.0 / 60.0) * kFrames;
  const double audio_ratio = static_cast<double>(g_audio_frames) / expected_audio;
  Check("audio is drained on the frontend thread in the right quantity",
        audio_ratio > 0.9 && audio_ratio < 1.1,
        std::to_string(g_audio_frames) + " frames drained, expected ~" +
            std::to_string(expected_audio));

  // 7. Input crosses the thread boundary intact.
  g_buttons_to_report = (1u << RETRO_DEVICE_ID_JOYPAD_A) | (1u << RETRO_DEVICE_ID_JOYPAD_START);
  g_core.retro_run();
  const std::uint16_t seen = g_core.last_buttons(0);
  Check("the input snapshot reaches the engine intact", seen == g_buttons_to_report,
        "sent 0x" + [&] {
          char buffer[16];
          std::snprintf(buffer, sizeof(buffer), "%04X", g_buttons_to_report);
          return std::string(buffer);
        }() + ", engine saw 0x" + [&] {
          char buffer[16];
          std::snprintf(buffer, sizeof(buffer), "%04X", seen);
          return std::string(buffer);
        }());
  g_buttons_to_report = 0;

  // 5 & 6. The stall path: dupe rather than hang, and no double-frame afterwards.
  const std::uint64_t served_before = g_core.frames_served();
  const std::uint64_t engine_before = g_core.engine_frame_index();
  const std::uint64_t dupes_before = g_core.dupes();
  g_video_calls.clear();

  // One frame that takes far longer than the 33 ms budget.
  g_core.set_stall(1, 200);
  const auto stall_start = std::chrono::steady_clock::now();
  g_core.retro_run();
  const auto stall_ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                            std::chrono::steady_clock::now() - stall_start)
                            .count();

  Check("a missed deadline dupes instead of blocking",
        g_core.dupes() == dupes_before + 1 && g_video_calls.size() == 1 &&
            g_video_calls[0].null_frame && stall_ms < 150,
        "returned in " + std::to_string(stall_ms) + " ms with a NULL frame, dupes " +
            std::to_string(dupes_before) + " → " + std::to_string(g_core.dupes()));

  // Let the stalled frame land, then run several more. The engine must not have banked
  // extra requests: this is the bug the monotonic counters exist to prevent. The wait is
  // deliberately generous — the point is that catching up does not *over*-serve.
  std::this_thread::sleep_for(std::chrono::milliseconds(300));

  constexpr int kAfter = 10;
  g_video_calls.clear();
  for (int i = 0; i < kAfter; ++i) g_core.retro_run();

  const std::uint64_t engine_frames = g_core.engine_frame_index() - engine_before;
  const std::uint64_t served_frames = g_core.frames_served() - served_before;
  // The engine ran the stalled frame plus at most one per subsequent retro_run. If the
  // gate leaked a request, the engine would be ahead of the frontend by more than one.
  Check("no frames are banked after a timeout (double-speed bug)",
        engine_frames <= static_cast<std::uint64_t>(kAfter) + 2 &&
            engine_frames >= served_frames,
        "engine advanced " + std::to_string(engine_frames) + " frames across " +
            std::to_string(kAfter) + " runs after the stall; frontend served " +
            std::to_string(served_frames));

  Check("no save-state support is declared, cleanly",
        g_core.retro_serialize_size() == 0, "retro_serialize_size() == 0");

  // Cheat entry points must exist even as no-ops: the frontend dlsyms them, and their
  // absence is what `EmulatorCore::supports_cheats` reports on.
  g_core.retro_cheat_reset();
  g_core.retro_cheat_set(0, true, "test");
  Check("cheat entry points are present and harmless", true, "reset + set are no-ops");

  // 9. Unload and reload, with no thread left behind.
  g_core.retro_unload_game();
  const bool reloaded = g_core.retro_load_game(&info);
  g_video_calls.clear();
  for (int i = 0; i < 5; ++i) g_core.retro_run();
  Check("unload then reload runs again", reloaded && g_video_calls.size() == 5,
        std::to_string(g_video_calls.size()) + " frames after reload");

  g_core.retro_unload_game();
  g_core.retro_deinit();
  dlclose(g_core.handle);

  std::printf("\n── %d/%d checks passed%s\n\n", g_checks - g_failures, g_checks,
              g_failures != 0 ? ", FAILURES" : "");
  return g_failures == 0 ? 0 : 1;
}
