#ifndef CONTINUUM_SYMBIAN_STUB_ENGINE_H
#define CONTINUUM_SYMBIAN_STUB_ENGINE_H

#include "symbian_engine.h"

namespace continuum {

class StubEngine final : public ISymbianEngine {
 public:
  bool EngineLinked() const override;
  const char* Refusal() const override;
  bool Initialise(const EngineConfig& config) override;
  bool InstallFirmware(const char* path) override;
  bool InstallPackage(const char* path) override;
  bool Boot() override;
  void Shutdown() override;
  ScreenInfo GetScreenInfo() const override;
  void SetKeys(std::uint32_t keys) override;
  void RunFrame() override;
};

}  // namespace continuum

#endif
