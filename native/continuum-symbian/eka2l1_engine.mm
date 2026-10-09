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
    refusal_ = "Continuum Symbian could not install this package (error " + std::to_string(err) +
               ").";
    // Not fatal: the phone menu still runs, so the user sees the firmware boot.
  }

  // Launch the app the package added. If it was already installed, match its name to the
  // file name; if nothing matches, stay on the phone's menu.
  std::string stem = package_.substr(package_.find_last_of('/') + 1);
  stem = Lower(stem.substr(0, stem.find_last_of('.')));
  std::uint32_t pick = 0;
  for (const auto& app : bridge::get_apps()) {
    if (before.count(app.uid) == 0) { pick = app.uid; break; }
  }
  if (pick == 0) {
    for (const auto& app : bridge::get_apps()) {
      const std::string name = Lower(app.name);
      if (!name.empty() && (stem.find(name) != std::string::npos ||
                            name.find(stem) != std::string::npos)) {
        pick = app.uid;
        break;
      }
    }
  }
  if (pick != 0) bridge::launch_app(pick);
  booted_ = true;
  return true;
}

void Eka2l1Engine::Shutdown() {
  continuum_symbian_frame_hook = nullptr;
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
