// Continuum Symbian — the only thing the libretro wrapper knows the emulator through.
//
// EKA2L1 is a separate program. This is the seam it has to sit behind. Until that program
// is linked, the only engine is the stub, and a .sis does not boot.

#ifndef CONTINUUM_SYMBIAN_ENGINE_H
#define CONTINUUM_SYMBIAN_ENGINE_H

#include <cstdint>

namespace continuum {

struct EngineConfig {
  const char* system_dir = nullptr;
  const char* save_dir = nullptr;
};

struct ScreenInfo {
  std::uint32_t width = 240;
  std::uint32_t height = 320;
  float aspect_ratio = 240.0f / 320.0f;
  double refresh_rate = 60.0;
  double sample_rate = 48000.0;
};

class ISymbianEngine {
 public:
  virtual ~ISymbianEngine() = default;

  // False until EKA2L1 is actually linked. The wrapper will not load a game while this
  // is false.
  virtual bool EngineLinked() const = 0;
  virtual const char* Refusal() const = 0;

  virtual bool Initialise(const EngineConfig& config) = 0;
  virtual bool InstallFirmware(const char* path) = 0;
  virtual bool InstallPackage(const char* path) = 0;
  virtual bool Boot() = 0;
  virtual void Shutdown() = 0;

  virtual ScreenInfo GetScreenInfo() const = 0;
  virtual void SetKeys(std::uint32_t keys) = 0;
  virtual void RunFrame() = 0;
};

}  // namespace continuum

#endif
