// Continuum — a libretro core that is really a wrapper around a standalone engine.
//
// Exports the standard `retro_*` surface, so as far as the rest of the system is concerned
// this is a core: `NativeCore` dlopens it, `CoreRegistry` holds it, `EmulatorCore`
// abstracts it, and `bridge.rs` never learns anything unusual happened.
//
// Step 10 of the Phase 5 sequence: the engine behind it is `StubEngine`, which renders a
// rotating colour. That is enough to exercise the whole wrapper — the frame gate, the
// thread-affinity rules, `set_image`, the Vulkan handover and the timeout-and-dupe path —
// with no emulator variables in play.
//
// The rule this file exists to enforce: libretro callbacks may only be used from the
// thread that called `retro_run`. Nothing inside the engine ever calls one. Frames arrive
// through the gate, audio through a ring drained here, input through a snapshot written
// here. See docs/SET_HW_RENDER_DESIGN.md §12.4.

#include <libretro.h>

#include <chrono>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include "frame_gate.h"
#include "stub_engine.h"
#include "switch_engine.h"

#ifdef CONTINUUM_HAVE_VULKAN
#include <libretro_vulkan.h>
#endif

namespace {

// ------------------------------------------------------------------- callbacks

retro_environment_t environ_cb = nullptr;
retro_video_refresh_t video_cb = nullptr;
retro_audio_sample_t audio_sample_cb = nullptr;
retro_audio_sample_batch_t audio_batch_cb = nullptr;
retro_input_poll_t input_poll_cb = nullptr;
retro_input_state_t input_state_cb = nullptr;
retro_log_printf_t log_cb = nullptr;

void Log(retro_log_level level, const char* format, ...) {
  char buffer[512];
  va_list args;
  va_start(args, format);
  std::vsnprintf(buffer, sizeof(buffer), format, args);
  va_end(args);
  if (log_cb != nullptr) {
    log_cb(level, "%s", buffer);
  } else {
    std::fprintf(stderr, "[switch-wrapper] %s", buffer);
  }
}

// ------------------------------------------------------------------- the core

class SwitchCore final : public continuum::PresentSink {
 public:
  SwitchCore() : engine_(std::make_unique<continuum::StubEngine>()) {}

  continuum::FrameGate& Gate() { return gate_; }
  continuum::ISwitchEngine& Engine() { return *engine_; }
  continuum::StubEngine& Stub() { return *engine_; }

  // ---- PresentSink: called on the engine's driver thread ----
  //
  // Records the frame and releases `retro_run`. It must not touch a libretro callback:
  // this is the wrong thread for that, which is the whole reason the gate exists.
  void OnEnginePresent(const continuum::PresentedFrame& frame) override {
    {
      std::lock_guard<std::mutex> lock(pending_mutex_);
      pending_ = frame;
      have_pending_ = true;
    }
    gate_.PublishFrame();
  }

  bool TakePending(continuum::PresentedFrame* out) {
    std::lock_guard<std::mutex> lock(pending_mutex_);
    if (!have_pending_) return false;
    *out = pending_;
    return true;
  }

  // ---- lifecycle ----

  bool Start(const continuum::EngineConfig& config, const char* content_path) {
    engine_->SetPresentSink(this);
    if (!engine_->Initialise(config)) return false;
    if (!engine_->MountContent(content_path)) return false;
    if (!engine_->BootTitle()) return false;

    gate_.Reset();
    running_ = true;
    driver_ = std::thread([this] { DriverThread(); });
    return true;
  }

  void Stop() {
    if (!running_) return;
    // Shut the gate first. Without this the driver thread parks in `AwaitRequest`
    // forever and the join below never returns.
    gate_.Shutdown();
    if (driver_.joinable()) driver_.join();
    engine_->Shutdown();
    running_ = false;
    have_pending_ = false;
    dupes_ = 0;
    frames_served_ = 0;
  }

  bool Running() const { return running_; }

  // ---- per-frame work, all on the frontend's thread ----

  void SnapshotInput() {
    continuum::InputSnapshot snapshot{};
    if (input_state_cb != nullptr) {
      for (unsigned port = 0; port < continuum::InputSnapshot::kMaxPlayers; ++port) {
        std::uint16_t buttons = 0;
        for (unsigned id = 0; id < 16; ++id) {
          if (input_state_cb(port, RETRO_DEVICE_JOYPAD, 0, id) != 0) {
            buttons |= static_cast<std::uint16_t>(1u << id);
          }
        }
        snapshot.buttons[port] = buttons;
        snapshot.left_x[port] = input_state_cb(port, RETRO_DEVICE_ANALOG,
                                               RETRO_DEVICE_INDEX_ANALOG_LEFT,
                                               RETRO_DEVICE_ID_ANALOG_X);
        snapshot.left_y[port] = input_state_cb(port, RETRO_DEVICE_ANALOG,
                                               RETRO_DEVICE_INDEX_ANALOG_LEFT,
                                               RETRO_DEVICE_ID_ANALOG_Y);
        snapshot.right_x[port] = input_state_cb(port, RETRO_DEVICE_ANALOG,
                                                RETRO_DEVICE_INDEX_ANALOG_RIGHT,
                                                RETRO_DEVICE_ID_ANALOG_X);
        snapshot.right_y[port] = input_state_cb(port, RETRO_DEVICE_ANALOG,
                                                RETRO_DEVICE_INDEX_ANALOG_RIGHT,
                                                RETRO_DEVICE_ID_ANALOG_Y);
      }
      // Handheld-mode touch, through the same pointer plumbing DS and 3DS use.
      snapshot.touch_pressed =
          input_state_cb(0, RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_PRESSED) != 0;
      snapshot.touch_x =
          input_state_cb(0, RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_X);
      snapshot.touch_y =
          input_state_cb(0, RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_Y);
    }
    engine_->SetInputSnapshot(snapshot);
  }

  void PumpAudio() {
    if (audio_batch_cb == nullptr) return;
    constexpr std::size_t kChunkFrames = 2048;
    std::int16_t buffer[kChunkFrames * 2];
    std::size_t frames;
    while ((frames = engine_->DrainAudio(buffer, kChunkFrames)) > 0) {
      audio_batch_cb(buffer, frames);
      if (frames < kChunkFrames) break;
    }
  }

  std::uint64_t Dupes() const { return dupes_; }
  void CountDupe() { ++dupes_; }
  std::uint64_t FramesServed() const { return frames_served_; }
  void CountServed() { ++frames_served_; }

 private:
  void DriverThread() {
    std::uint64_t served = 0;
    while (gate_.AwaitRequest(served)) {
      engine_->RunUntilPresent();  // publishes through OnEnginePresent
      ++served;
    }
  }

  std::unique_ptr<continuum::StubEngine> engine_;
  continuum::FrameGate gate_;
  std::thread driver_;
  bool running_ = false;

  std::mutex pending_mutex_;
  continuum::PresentedFrame pending_{};
  bool have_pending_ = false;

  std::uint64_t dupes_ = 0;
  std::uint64_t frames_served_ = 0;
};

std::unique_ptr<SwitchCore> g_core;

#ifdef CONTINUUM_HAVE_VULKAN
const retro_hw_render_interface_vulkan* g_vulkan_iface = nullptr;
retro_hw_render_callback g_hw_render{};
#endif

// A conservative default until the engine reports otherwise.
continuum::ScreenInfo g_screen{};

}  // namespace

// ============================================================ libretro surface

extern "C" {

RETRO_API unsigned retro_api_version(void) { return RETRO_API_VERSION; }

RETRO_API void retro_get_system_info(struct retro_system_info* info) {
  std::memset(info, 0, sizeof(*info));
  info->library_name = "Continuum Switch (stub)";
  info->library_version = "0.1.0-step10";
  // `need_fullpath` is true because Switch containers are mounted as filesystems rather
  // than read as a blob: an XCI is tens of gigabytes and the engine wants random access.
  info->need_fullpath = true;
  info->valid_extensions = "nsp|xci|nca|nro";
  info->block_extract = true;
}

RETRO_API void retro_get_system_av_info(struct retro_system_av_info* info) {
  g_screen = g_core != nullptr ? g_core->Engine().GetScreenInfo() : continuum::ScreenInfo{};
  std::memset(info, 0, sizeof(*info));
  info->geometry.base_width = g_screen.width;
  info->geometry.base_height = g_screen.height;
  // Declared generously: this sizes the frontend's target allocation, which cannot grow
  // mid-session, and a Switch title may move between handheld and docked resolutions.
  info->geometry.max_width = g_screen.max_width;
  info->geometry.max_height = g_screen.max_height;
  info->geometry.aspect_ratio = g_screen.aspect_ratio;
  info->timing.fps = g_screen.refresh_rate;
  info->timing.sample_rate = g_screen.sample_rate;
}

RETRO_API void retro_set_environment(retro_environment_t cb) {
  environ_cb = cb;

  bool no_game = false;
  cb(RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME, &no_game);

  retro_log_callback logging{};
  if (cb(RETRO_ENVIRONMENT_GET_LOG_INTERFACE, &logging)) log_cb = logging.log;

  // Per-extension content handling. The frontend already implements this — it was built
  // in Phase 1b for fceumm — so declaring it here costs nothing and is what makes the
  // path in `retro_load_game` reliable rather than hopeful.
  static const struct retro_system_content_info_override overrides[] = {
      {"nsp|xci|nca|nro", true /* need_fullpath */, false /* persistent_data */},
      {nullptr, false, false},
  };
  cb(RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE, (void*)overrides);

#ifdef CONTINUUM_HAVE_VULKAN
  // Ask for Vulkan, and register the negotiation interface so the engine is *given* a
  // device rather than creating one. On iOS there is exactly one GPU and every layer must
  // share the MTLDevice the Swift side made.
  g_hw_render.context_type = RETRO_HW_CONTEXT_VULKAN;
  g_hw_render.version_major = 1;
  g_hw_render.version_minor = 2;
  g_hw_render.context_reset = [] {
    const struct retro_hw_render_interface* iface = nullptr;
    if (environ_cb == nullptr ||
        !environ_cb(RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE, &iface) || iface == nullptr) {
      Log(RETRO_LOG_ERROR, "frontend provided no Vulkan render interface\n");
      return;
    }
    g_vulkan_iface = reinterpret_cast<const retro_hw_render_interface_vulkan*>(iface);
    if (g_vulkan_iface->interface_type != RETRO_HW_RENDER_INTERFACE_VULKAN) {
      Log(RETRO_LOG_ERROR, "render interface is not Vulkan\n");
      g_vulkan_iface = nullptr;
      return;
    }

    continuum::ExternalVulkanContext context{};
    context.instance = g_vulkan_iface->instance;
    context.physical_device = g_vulkan_iface->gpu;
    context.device = g_vulkan_iface->device;
    context.queue = g_vulkan_iface->queue;
    context.queue_family_index = g_vulkan_iface->queue_index;
    context.get_instance_proc = [](void* instance, const char* name) -> void* {
      return reinterpret_cast<void*>(g_vulkan_iface->get_instance_proc_addr(
          static_cast<VkInstance>(instance), name));
    };
    context.get_device_proc = [](void* device, const char* name) -> void* {
      return reinterpret_cast<void*>(
          g_vulkan_iface->get_device_proc_addr(static_cast<VkDevice>(device), name));
    };
    // A VkQueue is not thread-safe and under MoltenVK it is an MTLCommandQueue the
    // frontend also submits to. These are what make one shared queue safe.
    context.lock_queue = [](void* opaque) { g_vulkan_iface->lock_queue(opaque); };
    context.unlock_queue = [](void* opaque) { g_vulkan_iface->unlock_queue(opaque); };
    context.queue_lock_opaque = g_vulkan_iface->handle;

    if (g_core != nullptr) g_core->Engine().SetExternalVulkanContext(context);
    Log(RETRO_LOG_INFO, "Vulkan context adopted from the frontend\n");
  };
  g_hw_render.context_destroy = [] {
    g_vulkan_iface = nullptr;
    Log(RETRO_LOG_INFO, "Vulkan context released\n");
  };
  // False: we rebuild on loss rather than claiming the context survives it.
  g_hw_render.cache_context = false;
  // Vulkan's origin is top-left, like Metal's, so no flip is needed.
  g_hw_render.bottom_left_origin = false;
  g_hw_render.depth = true;
  g_hw_render.stencil = true;
  if (!cb(RETRO_ENVIRONMENT_SET_HW_RENDER, &g_hw_render)) {
    Log(RETRO_LOG_ERROR, "frontend refused RETRO_HW_CONTEXT_VULKAN\n");
  }
#endif
}

RETRO_API void retro_set_video_refresh(retro_video_refresh_t cb) { video_cb = cb; }
RETRO_API void retro_set_audio_sample(retro_audio_sample_t cb) { audio_sample_cb = cb; }
RETRO_API void retro_set_audio_sample_batch(retro_audio_sample_batch_t cb) {
  audio_batch_cb = cb;
}
RETRO_API void retro_set_input_poll(retro_input_poll_t cb) { input_poll_cb = cb; }
RETRO_API void retro_set_input_state(retro_input_state_t cb) { input_state_cb = cb; }

RETRO_API void retro_init(void) { g_core = std::make_unique<SwitchCore>(); }

RETRO_API void retro_deinit(void) {
  if (g_core != nullptr) g_core->Stop();
  g_core.reset();
}

RETRO_API void retro_set_controller_port_device(unsigned, unsigned) {}

RETRO_API bool retro_load_game(const struct retro_game_info* info) {
  if (g_core == nullptr) return false;

  // `need_fullpath` is declared, so a path is what should arrive. Saying so explicitly is
  // worth more than tolerating a null: a Switch container cannot be run from a blob, and
  // failing here names the reason.
  if (info == nullptr || info->path == nullptr) {
    Log(RETRO_LOG_ERROR, "Switch content must be a file path (need_fullpath)\n");
    return false;
  }

  const char* system_dir = nullptr;
  const char* save_dir = nullptr;
  if (environ_cb != nullptr) {
    environ_cb(RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY, &system_dir);
    environ_cb(RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY, &save_dir);
  }
  // Reported rather than fatal for the stub, which needs neither keys nor firmware. A
  // real engine validates them here and refuses with a `SET_MESSAGE` a user can act on.
  if (system_dir == nullptr) {
    Log(RETRO_LOG_WARN, "no system directory; keys and firmware would be unavailable\n");
  }

  continuum::EngineConfig config{};
  config.system_dir = system_dir;
  config.save_dir = save_dir;
  config.jit = true;
  config.memory_budget_bytes = 0;

  if (!g_core->Start(config, info->path)) {
    Log(RETRO_LOG_ERROR, "engine failed to start\n");
    return false;
  }
  g_screen = g_core->Engine().GetScreenInfo();
  Log(RETRO_LOG_INFO, "started: %s at %ux%u\n", info->path, g_screen.width, g_screen.height);
  return true;
}

RETRO_API bool retro_load_game_special(unsigned, const struct retro_game_info*, size_t) {
  return false;
}

RETRO_API void retro_unload_game(void) {
  if (g_core != nullptr) g_core->Stop();
}

RETRO_API unsigned retro_get_region(void) { return RETRO_REGION_NTSC; }

RETRO_API void retro_reset(void) {
  // A stub has nothing to reset, and restarting the driver thread would be a lie about
  // what happened. A real engine reboots the title here.
  Log(RETRO_LOG_INFO, "reset requested\n");
}

// The whole point of the wrapper, in one function.
RETRO_API void retro_run(void) {
  if (g_core == nullptr || !g_core->Running()) return;

  // 1. Input, once, snapshotted so the engine's threads never call back out.
  if (input_poll_cb != nullptr) input_poll_cb();
  g_core->SnapshotInput();

  // 2. Options the user changed since the last frame.
  bool updated = false;
  if (environ_cb != nullptr &&
      environ_cb(RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE, &updated) && updated) {
    // Nothing to reload in the stub; a real engine re-reads its variables here.
  }

  // 3. Advance exactly one frame, or give up and dupe.
  //
  // Two frame intervals of budget: tolerates jitter without letting a stall accumulate
  // latency. A miss is expected rather than exceptional — shader compilation stalls the
  // first time a title reaches new geometry — and duping keeps audio and input alive
  // where blocking would freeze both.
  const double fps = g_screen.refresh_rate > 0.0 ? g_screen.refresh_rate : 60.0;
  const auto budget = std::chrono::milliseconds(static_cast<int>(2000.0 / fps));

  if (g_core->Gate().PumpFrame(budget)) {
    continuum::PresentedFrame frame{};
    if (g_core->TakePending(&frame)) {
      g_core->CountServed();

#ifdef CONTINUUM_HAVE_VULKAN
      if (frame.image_view != nullptr && g_vulkan_iface != nullptr) {
        retro_vulkan_image image{};
        image.image_view = static_cast<VkImageView>(frame.image_view);
        image.image_layout = static_cast<VkImageLayout>(frame.image_layout);
        // `create_info` is what lets the frontend recreate a view if it needs its own.
        image.create_info.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
        image.create_info.image = static_cast<VkImage>(frame.image);
        image.create_info.viewType = VK_IMAGE_VIEW_TYPE_2D;
        image.create_info.format = VK_FORMAT_B8G8R8A8_UNORM;
        image.create_info.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};

        VkSemaphore wait = static_cast<VkSemaphore>(frame.wait_semaphore);
        const uint32_t wait_count = wait != VK_NULL_HANDLE ? 1u : 0u;
        // Order matters: the image must be handed over before the frame is announced.
        g_vulkan_iface->set_image(g_vulkan_iface->handle, &image, wait_count,
                                  wait_count != 0 ? &wait : nullptr,
                                  VK_QUEUE_FAMILY_IGNORED);
        video_cb(RETRO_HW_FRAME_BUFFER_VALID, frame.width, frame.height, 0);
      } else
#endif
      if (frame.software_pixels != nullptr) {
        // The stub's host renderer. Not a path a real Switch engine takes, but it is what
        // makes the gate and the contract testable without a GPU.
        video_cb(frame.software_pixels, frame.width, frame.height, frame.software_stride);
      } else {
        video_cb(nullptr, frame.width, frame.height, 0);
      }
    }
  } else {
    // NULL is a frame dupe by definition, not an error.
    g_core->CountDupe();
    video_cb(nullptr, g_screen.width, g_screen.height, 0);
  }

  // 4. Audio, on this thread, whichever branch ran.
  g_core->PumpAudio();
}

// ------------------------------------------------------------------ save state
//
// Zero means "unsupported", and the frontend already handles that cleanly:
// `EmulatorBridge::save_state` turns `state_size() == 0` into a legible error, and the
// player's Save button already reports it. Switch save states are not solved in any
// engine; savedata through GET_SAVE_DIRECTORY is what preserves progress.

RETRO_API size_t retro_serialize_size(void) { return 0; }
RETRO_API bool retro_serialize(void*, size_t) { return false; }
RETRO_API bool retro_unserialize(const void*, size_t) { return false; }

RETRO_API void retro_cheat_reset(void) {}
RETRO_API void retro_cheat_set(unsigned, bool, const char*) {}

RETRO_API void* retro_get_memory_data(unsigned) { return nullptr; }
RETRO_API size_t retro_get_memory_size(unsigned) { return 0; }

// ---------------------------------------------------------------- test hooks
//
// Not part of libretro. The harness uses these to assert on behaviour that is otherwise
// only observable as timing — how many frames were served versus duped, and whether the
// input snapshot crossed the thread boundary intact.

RETRO_API uint64_t continuum_stub_frames_served(void) {
  return g_core != nullptr ? g_core->FramesServed() : 0;
}

RETRO_API uint64_t continuum_stub_dupes(void) {
  return g_core != nullptr ? g_core->Dupes() : 0;
}

RETRO_API uint64_t continuum_stub_engine_frame_index(void) {
  return g_core != nullptr ? g_core->Stub().FrameIndex() : 0;
}

RETRO_API uint16_t continuum_stub_last_buttons(unsigned port) {
  if (g_core == nullptr || port >= continuum::InputSnapshot::kMaxPlayers) return 0;
  return g_core->Stub().LastInput().buttons[port];
}

RETRO_API void continuum_stub_set_stall(unsigned frames, unsigned millis) {
  if (g_core != nullptr) g_core->Stub().SetArtificialStall(frames, millis);
}

RETRO_API void continuum_stub_expected_colour(uint64_t frame_index, uint8_t* rgba) {
  continuum::StubExpectedColour(frame_index, rgba);
}

}  // extern "C"
