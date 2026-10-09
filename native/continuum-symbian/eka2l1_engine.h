// Continuum Symbian — the engine behind the seam, on the phone: EKA2L1's jitless iOS port
// (MuhannadYT/EKA2L1_IOS, dyncom interpreter). Only compiled for iOS (build-core.sh).
#ifndef CONTINUUM_SYMBIAN_EKA2L1_ENGINE_H
#define CONTINUUM_SYMBIAN_EKA2L1_ENGINE_H

#include "symbian_engine.h"

#include <cstdint>
#include <mutex>
#include <string>
#include <vector>

namespace continuum {

class Eka2l1Engine final : public ISymbianEngine {
 public:
  bool EngineLinked() const override { return true; }
  const char* Refusal() const override { return refusal_.c_str(); }

  bool Initialise(const EngineConfig& config) override;
  bool InstallFirmware(const char* path) override;
  bool InstallPackage(const char* path) override;
  bool Boot() override;
  void Shutdown() override;

  ScreenInfo GetScreenInfo() const override;
  void SetKeys(std::uint32_t keys) override;
  void RunFrame() override {}

  void Touch(int x, int y, int action);
  // Copies the newest frame (XRGB8888, top row first). False when none has arrived yet.
  bool LatestFrame(std::vector<std::uint32_t>& out, int& width, int& height);

 private:
  std::string data_dir_;
  std::string refusal_ = "Continuum Symbian is not initialised.";
  std::string package_;
  std::uint32_t keys_ = 0;
  void* layer_ = nullptr;
  bool booted_ = false;
};

}  // namespace continuum

#endif
