// Continuum — the interface the libretro wrapper knows the engine through.
//
// The wrapper is written against this and nothing else, so the engine underneath can be
// replaced, updated or stubbed without the libretro surface changing. Step 10 of the
// Phase 5 sequence implements `StubEngine`, which renders a rotating colour and nothing
// more — that exercises the gate, the thread affinity, the frame handover and the
// timeout path with zero emulator variables in play.
//
// See docs/SET_HW_RENDER_DESIGN.md §12.

#ifndef CONTINUUM_SWITCH_ENGINE_H
#define CONTINUUM_SWITCH_ENGINE_H

#include <cstddef>
#include <cstdint>

namespace continuum {

// ---------------------------------------------------------------------- input

// One frame's input, snapshotted by the wrapper on the frontend thread.
//
// Snapshotting rather than letting the engine call `input_state` has two reasons, and
// only the first is about safety: libretro callbacks may only be used from the thread
// that called `retro_run`, and the engine has threads of its own. The second is that a
// single immutable state per emulated frame is what makes frame timing reproducible.
struct InputSnapshot {
  static constexpr unsigned kMaxPlayers = 8;

  // Bit per libretro RETRO_DEVICE_ID_JOYPAD_* id.
  std::uint16_t buttons[kMaxPlayers] = {};
  std::int16_t left_x[kMaxPlayers] = {};
  std::int16_t left_y[kMaxPlayers] = {};
  std::int16_t right_x[kMaxPlayers] = {};
  std::int16_t right_y[kMaxPlayers] = {};

  bool touch_pressed = false;
  std::int16_t touch_x = 0;
  std::int16_t touch_y = 0;
};

// ------------------------------------------------------------------- geometry

struct ScreenInfo {
  std::uint32_t width = 0;
  std::uint32_t height = 0;
  // Declared up front and never exceeded: it sizes the frontend's target allocation,
  // which cannot grow mid-session.
  std::uint32_t max_width = 0;
  std::uint32_t max_height = 0;
  float aspect_ratio = 0.0f;
  double refresh_rate = 60.0;
  double sample_rate = 48000.0;
};

// ---------------------------------------------------------- Vulkan injection

// Everything the engine is given, and nothing it may create for itself.
//
// `lock_queue` / `unlock_queue` are not conveniences. A VkQueue is not thread-safe, and
// under MoltenVK a VkQueue *is* an MTLCommandQueue — the engine's GPU thread and the
// frontend's compositor both submit to it. Every engine submission must be wrapped.
struct ExternalVulkanContext {
  void* instance = nullptr;          // VkInstance
  void* physical_device = nullptr;   // VkPhysicalDevice
  void* device = nullptr;            // VkDevice
  void* queue = nullptr;             // VkQueue
  std::uint32_t queue_family_index = 0;

  void* (*get_instance_proc)(void* instance, const char* name) = nullptr;
  void* (*get_device_proc)(void* device, const char* name) = nullptr;

  void (*lock_queue)(void* opaque) = nullptr;
  void (*unlock_queue)(void* opaque) = nullptr;
  void* queue_lock_opaque = nullptr;
};

// A frame the engine finished. Handed to the frontend through `set_image`.
struct PresentedFrame {
  void* image = nullptr;        // VkImage
  void* image_view = nullptr;   // VkImageView
  void* wait_semaphore = nullptr;  // VkSemaphore, or null when nothing to wait on
  std::int32_t image_layout = 0;   // VkImageLayout
  std::uint32_t width = 0;
  std::uint32_t height = 0;

  // The stub's host renderer has no Vulkan objects, so it reports its colour here and
  // the wrapper takes the software path. Real engines leave this null.
  const std::uint8_t* software_pixels = nullptr;
  std::size_t software_stride = 0;
};

// The engine calls this instead of presenting to a swapchain of its own.
class PresentSink {
 public:
  virtual ~PresentSink() = default;
  virtual void OnEnginePresent(const PresentedFrame& frame) = 0;
};

// ------------------------------------------------------------------ the engine

struct EngineConfig {
  const char* system_dir = nullptr;  // keys, firmware
  const char* save_dir = nullptr;    // savedata
  bool jit = true;                   // unconditional; see the blueprint
  std::uint64_t memory_budget_bytes = 0;
};

class ISwitchEngine {
 public:
  virtual ~ISwitchEngine() = default;

  virtual bool Initialise(const EngineConfig& config) = 0;
  virtual bool MountContent(const char* path) = 0;
  virtual bool BootTitle() = 0;
  virtual void Shutdown() = 0;

  // Runs CPU and GPU until the engine reaches its presentation point. Blocking, and
  // called only from the driver thread the wrapper owns.
  virtual void RunUntilPresent() = 0;

  // Both injected before `Initialise`.
  virtual void SetExternalVulkanContext(const ExternalVulkanContext& context) = 0;
  virtual void SetPresentSink(PresentSink* sink) = 0;

  virtual void SetInputSnapshot(const InputSnapshot& snapshot) = 0;
  virtual std::size_t DrainAudio(std::int16_t* dst, std::size_t max_frames) = 0;
  virtual ScreenInfo GetScreenInfo() const = 0;

  // Step 10 only: lets the harness make the engine miss its deadline on purpose, so the
  // timeout-and-dupe path is tested rather than assumed.
  virtual void SetArtificialStall(unsigned frames, unsigned millis) {
    (void)frames;
    (void)millis;
  }
};

}  // namespace continuum

#endif  // CONTINUUM_SWITCH_ENGINE_H
