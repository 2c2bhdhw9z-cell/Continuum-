// Continuum — the Step 10 stub engine.
//
// Renders a rotating colour and nothing else. That is the entire point: it exercises the
// frame gate, the thread affinity rules, the frame handover and the timeout-and-dupe path
// with no emulator variables in play, so that every bug found when a real engine is wired
// in afterwards is unambiguously an engine-integration bug.
//
// Two renderers behind one interface:
//
//   HostStubRenderer    no Vulkan. Reports the colour as pixels. Runs anywhere, which is
//                       what makes the wrapper testable on a build host.
//   VulkanStubRenderer  vkCmdClearColorImage into a frontend-owned image, signalling a
//                       semaphore. Compiled when CONTINUUM_HAVE_VULKAN is defined.
//
// The colour walks around the HSV hue circle once every two seconds, so "is it rotating"
// is checkable by sampling two frames and comparing, and "is it rotating at the right
// rate" is checkable by counting frames per revolution.

#include "stub_engine.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstring>
#include <thread>

namespace continuum {
namespace {

// One revolution of the hue circle, in emulated frames at 60 Hz.
constexpr double kFramesPerRevolution = 120.0;

// Fully saturated hue → RGB. Kept explicit rather than pulled from a colour library so
// the harness can reproduce the expected value independently.
void HueToRgb(double hue, std::uint8_t* out) {
  const double h = std::fmod(std::fmax(hue, 0.0), 1.0) * 6.0;
  const int sector = static_cast<int>(h);
  const double f = h - sector;
  const std::uint8_t v = 255;
  const std::uint8_t p = 0;
  const std::uint8_t q = static_cast<std::uint8_t>(255.0 * (1.0 - f));
  const std::uint8_t t = static_cast<std::uint8_t>(255.0 * f);

  switch (sector % 6) {
    case 0: out[0] = v; out[1] = t; out[2] = p; break;
    case 1: out[0] = q; out[1] = v; out[2] = p; break;
    case 2: out[0] = p; out[1] = v; out[2] = t; break;
    case 3: out[0] = p; out[1] = q; out[2] = v; break;
    case 4: out[0] = t; out[1] = p; out[2] = v; break;
    default: out[0] = v; out[1] = p; out[2] = q; break;
  }
  out[3] = 255;
}

}  // namespace

// ---------------------------------------------------------------- host renderer

void HostStubRenderer::Configure(std::uint32_t width, std::uint32_t height) {
  width_ = width;
  height_ = height;
  // One pixel row is enough for the harness to sample, but a full surface keeps the
  // software path honest about stride arithmetic.
  pixels_.assign(static_cast<std::size_t>(width) * height * 4, 0);
}

void HostStubRenderer::Render(std::uint64_t frame_index, PresentedFrame* out) {
  std::uint8_t rgba[4];
  HueToRgb(static_cast<double>(frame_index) / kFramesPerRevolution, rgba);

  for (std::size_t i = 0; i < pixels_.size(); i += 4) {
    pixels_[i + 0] = rgba[0];
    pixels_[i + 1] = rgba[1];
    pixels_[i + 2] = rgba[2];
    pixels_[i + 3] = rgba[3];
  }

  out->software_pixels = pixels_.data();
  out->software_stride = static_cast<std::size_t>(width_) * 4;
  out->width = width_;
  out->height = height_;
}

// -------------------------------------------------------------------- the engine

StubEngine::StubEngine() = default;
StubEngine::~StubEngine() { Shutdown(); }

bool StubEngine::Initialise(const EngineConfig& config) {
  config_ = config;

  // The renderer is chosen by what the frontend actually gave us, not by a build flag:
  // a Vulkan context means the real handover path, its absence means the host path.
#ifdef CONTINUUM_HAVE_VULKAN
  if (vulkan_.device != nullptr) {
    vulkan_renderer_ = std::make_unique<VulkanStubRenderer>();
    if (!vulkan_renderer_->Initialise(vulkan_)) {
      vulkan_renderer_.reset();
      return false;
    }
    vulkan_renderer_->Configure(kStubWidth, kStubHeight);
    initialised_ = true;
    return true;
  }
#endif

  host_renderer_ = std::make_unique<HostStubRenderer>();
  host_renderer_->Configure(kStubWidth, kStubHeight);
  initialised_ = true;
  return true;
}

bool StubEngine::MountContent(const char* path) {
  // A stub has no content. Recording the path is still worth doing: it is what proves
  // `need_fullpath` and `GET_GAME_INFO_EXT` delivered a usable path rather than null,
  // which is the first thing to break when a real container is mounted.
  mounted_path_ = path != nullptr ? path : "";
  return true;
}

bool StubEngine::BootTitle() {
  frame_index_ = 0;
  return initialised_;
}

void StubEngine::Shutdown() {
#ifdef CONTINUUM_HAVE_VULKAN
  if (vulkan_renderer_) {
    vulkan_renderer_->Shutdown();
    vulkan_renderer_.reset();
  }
#endif
  host_renderer_.reset();
  initialised_ = false;
}

void StubEngine::RunUntilPresent() {
  if (!initialised_ || sink_ == nullptr) return;

  // Deliberate deadline miss, so the wrapper's dupe path is tested rather than assumed.
  // A real engine misses for the same reason with different timing: shader compilation.
  if (stall_frames_ > 0) {
    --stall_frames_;
    std::this_thread::sleep_for(std::chrono::milliseconds(stall_millis_));
  }

  PresentedFrame frame{};

#ifdef CONTINUUM_HAVE_VULKAN
  if (vulkan_renderer_) {
    vulkan_renderer_->Render(frame_index_, &frame);
  } else
#endif
  if (host_renderer_) {
    host_renderer_->Render(frame_index_, &frame);
  }

  ++frame_index_;

  // Audio: 48 kHz stereo silence at one frame's worth per frame. Silence is correct for a
  // stub, but the *quantity* matters — it is what proves the ring and the drain on the
  // frontend thread keep up, and a mismatch here shows as underruns in the existing
  // telemetry rather than as anything audible.
  const std::size_t frames_of_audio =
      static_cast<std::size_t>(48000.0 / GetScreenInfo().refresh_rate);
  {
    std::lock_guard<std::mutex> lock(audio_mutex_);
    audio_.insert(audio_.end(), frames_of_audio * 2, 0);
    // Bound it: if the frontend ever stops draining, this must not grow without limit.
    const std::size_t cap = 48000 * 2;  // one second
    if (audio_.size() > cap) {
      audio_.erase(audio_.begin(), audio_.begin() + (audio_.size() - cap));
    }
  }

  // This is the inversion: publishing is what releases `retro_run`, and the engine's
  // driver thread then parks in `AwaitRequest` until the frontend wants another.
  sink_->OnEnginePresent(frame);
}

void StubEngine::SetExternalVulkanContext(const ExternalVulkanContext& context) {
  vulkan_ = context;
}

void StubEngine::SetPresentSink(PresentSink* sink) { sink_ = sink; }

void StubEngine::SetInputSnapshot(const InputSnapshot& snapshot) {
  // Stored under a lock rather than assigned, because a real engine reads this from its
  // own HID thread. The stub reads it back only so the harness can prove the snapshot
  // crossed intact.
  std::lock_guard<std::mutex> lock(input_mutex_);
  input_ = snapshot;
}

InputSnapshot StubEngine::LastInput() const {
  std::lock_guard<std::mutex> lock(input_mutex_);
  return input_;
}

std::size_t StubEngine::DrainAudio(std::int16_t* dst, std::size_t max_frames) {
  std::lock_guard<std::mutex> lock(audio_mutex_);
  const std::size_t available_frames = audio_.size() / 2;
  const std::size_t take = std::min(available_frames, max_frames);
  if (take == 0) return 0;
  std::memcpy(dst, audio_.data(), take * 2 * sizeof(std::int16_t));
  audio_.erase(audio_.begin(), audio_.begin() + take * 2);
  return take;
}

ScreenInfo StubEngine::GetScreenInfo() const {
  ScreenInfo info{};
  info.width = kStubWidth;
  info.height = kStubHeight;
  // Docked 1080p is the ceiling a real Switch engine would declare, and it sizes the
  // frontend's target allocation — which cannot grow mid-session, so the stub declares
  // the same maximum it eventually needs rather than its own small size.
  info.max_width = 1920;
  info.max_height = 1080;
  info.aspect_ratio = 16.0f / 9.0f;
  info.refresh_rate = 60.0;
  info.sample_rate = 48000.0;
  return info;
}

void StubEngine::SetArtificialStall(unsigned frames, unsigned millis) {
  stall_frames_ = frames;
  stall_millis_ = millis;
}

// Exposed for the harness so it can compute the same colour independently rather than
// trusting the engine's own report.
void StubExpectedColour(std::uint64_t frame_index, std::uint8_t* rgba) {
  HueToRgb(static_cast<double>(frame_index) / kFramesPerRevolution, rgba);
}

}  // namespace continuum
