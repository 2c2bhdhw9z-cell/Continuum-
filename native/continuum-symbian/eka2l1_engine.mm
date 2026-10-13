// Continuum Symbian on the phone. See eka2l1_engine.h.
//
// The emulator renders into an off-screen CAEAGLLayer on its own graphics thread. The Continuum
// patch to context_eagl.mm calls continuum_symbian_frame_hook before every present, where the
// frame is read back with glReadPixels and kept for retro_run. No JIT: the iOS port's default
// CPU is the dyncom interpreter and nothing here turns dynarmic on.

#include "eka2l1_engine.h"

#import <Foundation/Foundation.h>
#import <QuartzCore/CAEAGLLayer.h>
#import <OpenGLES/EAGLDrawable.h>
#import <OpenGLES/ES3/gl.h>

#include <ios/emu_bridge.h>
#include <miniz.h>

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <set>
#include <sys/stat.h>

extern "C" void (*continuum_symbian_frame_hook)(int width, int height);
// The emulator's assets (GLES shaders, compat list, Symbian patch DLLs), zipped into this
// dylib by build-core.sh (assets.s).
extern "C" const unsigned char continuum_symbian_assets[];
extern "C" const unsigned char continuum_symbian_assets_end[];

namespace continuum {
namespace {

namespace bridge = eka2l1::ios::bridge;

constexpr int kWidth = 240;
constexpr int kHeight = 320;

std::mutex g_frame_mutex;
std::vector<std::uint32_t> g_frame;
std::vector<std::uint32_t> g_scratch;
int g_frame_w = 0;
int g_frame_h = 0;
bool g_have_frame = false;

void FrameHook(int width, int height) {
  if (width <= 0 || height <= 0) return;
  g_scratch.resize(static_cast<size_t>(width) * height);
  glPixelStorei(GL_PACK_ALIGNMENT, 4);
  glReadPixels(0, 0, width, height, GL_RGBA, GL_UNSIGNED_BYTE, g_scratch.data());
  std::lock_guard<std::mutex> lock(g_frame_mutex);
  g_frame.resize(g_scratch.size());
  // GL rows are bottom first; RGBA bytes become XRGB8888.
  for (int y = 0; y < height; ++y) {
    const std::uint32_t* src = g_scratch.data() + static_cast<size_t>(height - 1 - y) * width;
    std::uint32_t* dst = g_frame.data() + static_cast<size_t>(y) * width;
    for (int x = 0; x < width; ++x) {
      const std::uint32_t p = src[x];
      const std::uint32_t r = p & 0xFF, g = (p >> 8) & 0xFF, b = (p >> 16) & 0xFF;
      dst[x] = (r << 16) | (g << 8) | b;
    }
  }
  g_frame_w = width;
  g_frame_h = height;
  g_have_frame = true;
}

void MakeDirs(const std::string& path) {
  [[NSFileManager defaultManager] createDirectoryAtPath:[NSString stringWithUTF8String:path.c_str()]
                            withIntermediateDirectories:YES
                                             attributes:nil
                                                  error:nil];
}

// Unzips the embedded assets into <bundle>/assets once; bridge::set_data_directory copies
// them from there into the data directory the way the iOS port's own app does.
bool StageAssets(const std::string& bundle) {
  const std::string marker = bundle + "/assets/.continuum-staged";
  struct stat st {};
  if (stat(marker.c_str(), &st) == 0) return true;
  const size_t size =
      static_cast<size_t>(continuum_symbian_assets_end - continuum_symbian_assets);
  mz_zip_archive zip{};
  if (!mz_zip_reader_init_mem(&zip, continuum_symbian_assets, size, 0)) return false;
  const mz_uint count = mz_zip_reader_get_num_files(&zip);
  bool ok = true;
  for (mz_uint i = 0; i < count && ok; ++i) {
    mz_zip_archive_file_stat fs{};
    if (!mz_zip_reader_file_stat(&zip, i, &fs)) { ok = false; break; }
    const std::string out = bundle + "/assets/" + fs.m_filename;
    if (mz_zip_reader_is_file_a_directory(&zip, i)) { MakeDirs(out); continue; }
    MakeDirs(out.substr(0, out.find_last_of('/')));
    ok = mz_zip_reader_extract_to_file(&zip, i, out.c_str(), 0);
  }
  mz_zip_reader_end(&zip);
  if (ok) {
    FILE* f = std::fopen(marker.c_str(), "w");
    if (f) std::fclose(f);
  }
  return ok;
}

std::string Lower(std::string s) {
  std::transform(s.begin(), s.end(), s.begin(), [](unsigned char c) { return std::tolower(c); });
  return s;
}

bool EndsWith(const std::string& s, const char* suffix) {
  const size_t n = std::strlen(suffix);
  return s.size() >= n && s.compare(s.size() - n, n, suffix) == 0;
}

}  // namespace

bool Eka2l1Engine::Initialise(const EngineConfig& config) {
  if (config.system_dir == nullptr) {
    refusal_ = "Continuum Symbian got no system directory from the app.";
    return false;
  }
  // <system>/continuum-symbian holds everything: the staged assets, the installed device
  // (ROM/RPKG), the C/D/E drives and the config. Kept across launches.
  const std::string root = std::string(config.system_dir) + "/continuum-symbian";
  root_dir_ = root;
  data_dir_ = root + "/data";
  MakeDirs(data_dir_);
  if (!StageAssets(root)) {
    refusal_ = "Continuum Symbian could not unpack its own assets.";
    return false;
  }
  // NOTE: this sets the process working directory, as the iOS port's own app does.
  bridge::set_data_directory(data_dir_, root);
  return true;
}

// Firmware is looked for in <system>/continuum-symbian/firmware: either a device dump
// (a .rom, plus an optional .rpkg next to it) or a .vpl with its .fpsx/.rofs files.
bool Eka2l1Engine::InstallFirmware(const char* dir) {
  if (bridge::has_device() || !bridge::get_devices().empty()) return true;
  NSArray<NSString*>* files =
      [[NSFileManager defaultManager] contentsOfDirectoryAtPath:[NSString stringWithUTF8String:dir]
                                                          error:nil];
  std::string rom, rpkg, vpl;
  for (NSString* name in files) {
    const std::string n = [name UTF8String];
    const std::string l = Lower(n);
    const std::string full = std::string(dir) + "/" + n;
    if (EndsWith(l, ".rom")) rom = full;
    else if (EndsWith(l, ".rpkg")) rpkg = full;
    else if (EndsWith(l, ".vpl")) vpl = full;
  }
  int err = -1;
  if (!rom.empty()) err = bridge::install_device(rpkg, rom, !rpkg.empty());
  else if (!vpl.empty()) err = bridge::install_device("", vpl, false);
  if (err != 0) {
    refusal_ = rom.empty() && vpl.empty()
                   ? "No Symbian firmware. Put a device dump (.rom, and its .rpkg if you have one) "
                     "in system/continuum-symbian/firmware."
                   : "The Symbian firmware in system/continuum-symbian/firmware did not install "
                     "(error " + std::to_string(err) + ").";
    return false;
  }
  return true;
}

bool Eka2l1Engine::InstallPackage(const char* path) {
  package_ = path != nullptr ? path : "";
  return !package_.empty();
}

// Installing a package is the slow part of starting a Symbian game, and it only has to happen
// once: a .sis rewrites every file it ships, so doing it again on every launch wastes a minute
// AND can write over a save a package carries. What was installed is remembered by the file's
// path, size and date, and an install is skipped only when that record matches AND the app is
// really in the phone's list, so a record that has gone stale costs nothing.
void Eka2l1Engine::InstallAndLaunch() {
  const std::string stamp_path = root_dir_ + "/installed.txt";
  struct stat st {};
  std::string stamp = package_;
  if (stat(package_.c_str(), &st) == 0) {
    stamp += "|" + std::to_string(static_cast<long long>(st.st_size)) + "|" +
             std::to_string(static_cast<long long>(st.st_mtime));
  }

  const std::string stem_full = package_.substr(package_.find_last_of('/') + 1);
  const std::string stem = Lower(stem_full.substr(0, stem_full.find_last_of('.')));
  auto find_by_name = [&stem]() -> std::uint32_t {
    for (const auto& app : bridge::get_apps()) {
      const std::string name = Lower(app.name);
      if (!name.empty() && (stem.find(name) != std::string::npos ||
                            name.find(stem) != std::string::npos)) {
        return app.uid;
      }
    }
    return 0;
  };

  bool installed_before = false;
  if (FILE* f = std::fopen(stamp_path.c_str(), "r")) {
    char line[2048] = {};
    if (std::fgets(line, sizeof(line), f) != nullptr) {
      std::string recorded(line);
      while (!recorded.empty() && (recorded.back() == '\n' || recorded.back() == '\r')) {
        recorded.pop_back();
      }
      installed_before = recorded == stamp;
    }
    std::fclose(f);
  }

  std::uint32_t pick = 0;
  if (installed_before) {
    pick = find_by_name();
  }
  if (pick == 0) {
    std::set<std::uint32_t> before;
    for (const auto& app : bridge::get_apps()) before.insert(app.uid);
    const std::string lower = Lower(package_);
    std::string game_name;
    int err = 0;
    if (EndsWith(lower, ".n-gage")) {
      err = bridge::install_ngage_file(package_, game_name);
    } else {
      err = bridge::install_app(package_);
    }
    if (err != 0) {
      // Not fatal: the phone's own menu is running, so the player still sees something. The
      // refusal string is not touched from this thread, because the launch has already
      // returned by now and the main thread may be reading it.
      std::fprintf(stderr, "[Continuum Symbian] install failed, error %d\n", err);
    } else if (FILE* f = std::fopen(stamp_path.c_str(), "w")) {
      std::fprintf(f, "%s\n", stamp.c_str());
      std::fclose(f);
    }
    for (const auto& app : bridge::get_apps()) {
      if (before.count(app.uid) == 0) { pick = app.uid; break; }
    }
    if (pick == 0) pick = find_by_name();
  }
  // Nothing matched: stay on the phone's own menu rather than guessing at an app.
  if (pick != 0) bridge::launch_app(pick);
  installing_ = false;
  // A pause while this was running was only remembered, not sent, so that it could not land in
  // the middle of an install. Send it now.
  if (paused_) bridge::pause();
}

bool Eka2l1Engine::Boot() {
  CAEAGLLayer* layer = [CAEAGLLayer layer];
  layer.frame = CGRectMake(0, 0, kWidth, kHeight);
  layer.contentsScale = 1.0;
  layer.opaque = YES;
  layer.drawableProperties = @{
    kEAGLDrawablePropertyRetainedBacking : @(YES),
    kEAGLDrawablePropertyColorFormat : kEAGLColorFormatRGBA8
  };
  layer_ = const_cast<void*>(CFRetain((__bridge CFTypeRef)layer));
  continuum_symbian_frame_hook = &FrameHook;

  if (!bridge::start(layer_, kWidth, kHeight)) {
    refusal_ = "The Symbian firmware is installed but did not boot.";
    return false;
  }

  booted_ = true;
  // Off this thread: the app's launch runs on the main thread, and the bridge itself says to
  // install from a background one. The phone's own menu is on screen while it happens.
  installing_ = true;
  install_thread_ = std::thread([this]() { InstallAndLaunch(); });
  return true;
}

// The player paused, left the game, or put the app in the background. Without this the guest
// keeps running and its sound keeps playing, because it is not driven by retro_run.
void Eka2l1Engine::SetPaused(bool paused) {
  if (!booted_ || paused == paused_) return;
  paused_ = paused;
  // While a package is installing, the choice is only remembered: the install thread applies it
  // when it is done, so a pause cannot land in the middle of writing the package's files.
  if (installing_) return;
  if (paused) {
    bridge::pause();
  } else {
    bridge::resume();
  }
}

void Eka2l1Engine::Shutdown() {
  if (install_thread_.joinable()) install_thread_.join();
  paused_ = false;
  continuum_symbian_frame_hook = nullptr;
  {
    // The next game must not open on the last game's picture.
    std::lock_guard<std::mutex> lock(g_frame_mutex);
    g_have_frame = false;
    g_frame.clear();
  }
  if (booted_) bridge::shutdown();
  booted_ = false;
  if (layer_ != nullptr) {
    CFRelease(layer_);
    layer_ = nullptr;
  }
}

ScreenInfo Eka2l1Engine::GetScreenInfo() const {
  ScreenInfo s;
  s.width = kWidth;
  s.height = kHeight;
  s.aspect_ratio = static_cast<float>(kWidth) / kHeight;
  return s;
}

// Bits are the libretro joypad ids. Each maps to the Symbian scancode the iOS port's own
// on-screen pad sends.
void Eka2l1Engine::SetKeys(std::uint32_t keys) {
  static const struct { int bit; int code; } kMap[] = {
      {4, 0x10},  {5, 0x11},  {6, 0x0E},  {7, 0x0F},   // up down left right
      {8, 0xA7},                                       // A: centre / fire
      {0, '5'},   {1, '1'},   {9, '3'},                // B 5, Y 1, X 3
      {12, '7'},  {13, '9'},                           // L2 7, R2 9
      {10, '*'},  {11, 0x7F},                          // L *, R #
      {2, 0xA4},  {3, 0xA5},                           // Select / Start: soft keys
  };
  const std::uint32_t changed = keys ^ keys_;
  for (const auto& m : kMap) {
    if (changed & (1u << m.bit)) bridge::key(m.code, (keys & (1u << m.bit)) != 0);
  }
  keys_ = keys;
}

void Eka2l1Engine::Touch(int x, int y, int action) {
  bridge::touch(x, y, static_cast<bridge::touch_action>(action), 0);
}

bool Eka2l1Engine::LatestFrame(std::vector<std::uint32_t>& out, int& width, int& height) {
  std::lock_guard<std::mutex> lock(g_frame_mutex);
  if (!g_have_frame) return false;
  out = g_frame;
  width = g_frame_w;
  height = g_frame_h;
  return true;
}

}  // namespace continuum
