#include "stub_engine.h"

namespace continuum {

bool StubEngine::EngineLinked() const { return false; }

const char* StubEngine::Refusal() const {
  return "Continuum Symbian has no emulator linked yet. A .sis will not boot.";
}

bool StubEngine::Initialise(const EngineConfig&) { return false; }
bool StubEngine::InstallFirmware(const char*) { return false; }
bool StubEngine::InstallPackage(const char*) { return false; }
bool StubEngine::Boot() { return false; }
void StubEngine::Shutdown() {}
ScreenInfo StubEngine::GetScreenInfo() const { return ScreenInfo{}; }
void StubEngine::SetKeys(std::uint32_t) {}
void StubEngine::RunFrame() {}

}  // namespace continuum
