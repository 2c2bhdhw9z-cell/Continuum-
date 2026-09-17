// Continuum — Step 10 stub engine declarations. See stub_engine.cpp.

#ifndef CONTINUUM_STUB_ENGINE_H
#define CONTINUUM_STUB_ENGINE_H

#include <cstdint>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

#include "switch_engine.h"

#ifdef CONTINUUM_HAVE_VULKAN
#include "vulkan_stub_renderer.h"
#endif

namespace continuum {

// 720p: large enough that stride arithmetic is exercised, small enough to clear cheaply.
constexpr std::uint32_t kStubWidth = 1280;
constexpr std::uint32_t kStubHeight = 720;

// No Vulkan. Reports the rotating colour as pixels, so the wrapper's gate, threading and
// libretro contract are all testable on a build host with no GPU.
class HostStubRenderer {
 public:
  void Configure(std::uint32_t width, std::uint32_t height);
  void Render(std::uint64_t frame_index, PresentedFrame* out);

 private:
  std::uint32_t width_ = 0;
  std::uint32_t height_ = 0;
  std::vector<std::uint8_t> pixels_;
};

class StubEngine final : public ISwitchEngine {
 public:
  StubEngine();
  ~StubEngine() override;

  bool Initialise(const EngineConfig& config) override;
  bool MountContent(const char* path) override;
  bool BootTitle() override;
  void Shutdown() override;

  void RunUntilPresent() override;

  void SetExternalVulkanContext(const ExternalVulkanContext& context) override;
  void SetPresentSink(PresentSink* sink) override;

  void SetInputSnapshot(const InputSnapshot& snapshot) override;
  std::size_t DrainAudio(std::int16_t* dst, std::size_t max_frames) override;
  ScreenInfo GetScreenInfo() const override;
  void SetArtificialStall(unsigned frames, unsigned millis) override;

  // Harness seams.
  InputSnapshot LastInput() const;
  const std::string& MountedPath() const { return mounted_path_; }
  std::uint64_t FrameIndex() const { return frame_index_; }

 private:
  EngineConfig config_{};
  ExternalVulkanContext vulkan_{};
  PresentSink* sink_ = nullptr;
  bool initialised_ = false;
  std::uint64_t frame_index_ = 0;
  std::string mounted_path_;

  unsigned stall_frames_ = 0;
  unsigned stall_millis_ = 0;

  std::unique_ptr<HostStubRenderer> host_renderer_;
#ifdef CONTINUUM_HAVE_VULKAN
  std::unique_ptr<VulkanStubRenderer> vulkan_renderer_;
#endif

  mutable std::mutex input_mutex_;
  InputSnapshot input_{};

  std::mutex audio_mutex_;
  std::vector<std::int16_t> audio_;
};

// The colour the stub will produce for a given frame, so a test can assert against an
// independently computed value rather than against the engine's own output.
void StubExpectedColour(std::uint64_t frame_index, std::uint8_t* rgba);

}  // namespace continuum

#endif  // CONTINUUM_STUB_ENGINE_H
