// Loads the core the way the app will, and checks two things: the name is Continuum
// Symbian, and a .sis does not boot while the emulator is not linked.

#include <dlfcn.h>

#include <cstdio>
#include <cstring>
#include <string>

#include <libretro.h>

namespace {

int g_failures = 0;

void Check(const char* name, bool ok, const char* detail) {
  if (ok) {
    std::printf("PASS  %s%s%s\n", name, detail[0] ? " — " : "", detail);
  } else {
    ++g_failures;
    std::printf("FAIL  %s%s%s\n", name, detail[0] ? " — " : "", detail);
  }
}

bool Environ(unsigned cmd, void* data) {
  if (cmd == RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME) return true;
  if (cmd == RETRO_ENVIRONMENT_GET_LOG_INTERFACE) {
    (void)data;
    return false;
  }
  return false;
}

bool Contains(const char* haystack, const char* needle) {
  return haystack != nullptr && std::strstr(haystack, needle) != nullptr;
}

}  // namespace

int main(int argc, char** argv) {
  if (argc != 2) {
    std::fprintf(stderr, "usage: continuum_symbian_test <core>\n");
    return 2;
  }
  void* lib = dlopen(argv[1], RTLD_NOW);
  if (lib == nullptr) {
    std::fprintf(stderr, "dlopen: %s\n", dlerror());
    return 2;
  }

  auto get_info = reinterpret_cast<void (*)(retro_system_info*)>(
      dlsym(lib, "retro_get_system_info"));
  auto set_env = reinterpret_cast<void (*)(retro_environment_t)>(
      dlsym(lib, "retro_set_environment"));
  auto init = reinterpret_cast<void (*)()>(dlsym(lib, "retro_init"));
  auto deinit = reinterpret_cast<void (*)()>(dlsym(lib, "retro_deinit"));
  auto load = reinterpret_cast<bool (*)(const retro_game_info*)>(dlsym(lib, "retro_load_game"));
  auto linked = reinterpret_cast<int (*)()>(dlsym(lib, "continuum_symbian_engine_linked"));

  Check("symbols", get_info && set_env && init && deinit && load && linked, "");

  retro_system_info info{};
  if (get_info != nullptr) get_info(&info);
  Check("name is Continuum Symbian",
        info.library_name != nullptr && std::strcmp(info.library_name, "Continuum Symbian") == 0,
        info.library_name != nullptr ? info.library_name : "null");
  Check("not named EKA2L1",
        info.library_name == nullptr || std::strstr(info.library_name, "EKA2L1") == nullptr, "");
  Check("not named libretro",
        info.library_name == nullptr || std::strstr(info.library_name, "libretro") == nullptr, "");
  Check("sis", Contains(info.valid_extensions, "sis"), info.valid_extensions ? info.valid_extensions : "");
  Check("sisx", Contains(info.valid_extensions, "sisx"), "");
  Check("full path", info.need_fullpath, "");
  Check("do not unzip for it", info.block_extract, "");

  if (set_env != nullptr) set_env(Environ);
  if (init != nullptr) init();
  Check("emulator is not linked", linked == nullptr || linked() == 0, "");

  retro_game_info game{};
  game.path = "game.sis";
  bool booted = load != nullptr && load(&game);
  Check("a sis does not boot", !booted, "");

  if (deinit != nullptr) deinit();
  dlclose(lib);

  if (g_failures != 0) {
    std::printf("%d failed\n", g_failures);
    return 1;
  }
  std::printf("all passed\n");
  return 0;
}
