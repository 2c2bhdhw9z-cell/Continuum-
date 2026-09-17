// Continuum — inversion of control between a standalone engine and libretro.
//
// A standalone emulator owns its loop. libretro requires the frontend to own it:
// `retro_run()` must advance exactly one frame, synchronously, and return.
//
// Rather than restructure an engine into a step function — which is a permanent fork of
// its scheduler — this installs a rendezvous at its presentation point. The engine keeps
// its threads and its loop; its `Present` call blocks until the frontend asks for another
// frame.
//
// See docs/SET_HW_RENDER_DESIGN.md §12.1.

#ifndef CONTINUUM_FRAME_GATE_H
#define CONTINUUM_FRAME_GATE_H

#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <mutex>

namespace continuum {

class FrameGate {
 public:
  // ---------------------------------------------------------------- frontend side

  // Called from `retro_run`. Asks for one frame and waits for it.
  //
  // Returns false if the frame did not arrive within `budget`, in which case the caller
  // must dupe — pass NULL to `video_refresh`, which libretro defines as "repeat the last
  // frame". That is not an error path: shader compilation legitimately stalls a frame for
  // hundreds of milliseconds, and blocking `retro_run` for that long freezes audio and
  // input as well.
  bool PumpFrame(std::chrono::milliseconds budget) {
    std::uint64_t target;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      // At most one frame in flight, and `requested_` is recomputed from `completed_`
      // rather than incremented.
      //
      // This is the subtle part, and getting it wrong is silent. A naive `++requested_`
      // leaks a request on every timeout: the engine eventually catches up, the counter
      // is now two ahead, and it runs two frames for the next single `retro_run`. The
      // frontend then sees one frame for every two emulated — the game runs at double
      // speed with half its frames displayed, which reads as "emulation is too fast"
      // rather than as a synchronisation bug.
      target = completed_ + 1;
      if (requested_ < target) requested_ = target;
    }
    cv_.notify_all();

    std::unique_lock<std::mutex> lock(mutex_);
    const bool arrived = cv_.wait_for(lock, budget, [this, target] {
      return completed_ >= target || quitting_;
    });
    return arrived && !quitting_;
  }

  // ------------------------------------------------------------------ engine side

  // Blocks the engine's driver thread until the frontend wants frame `served + 1`.
  // Returns false when shutting down, which is the driver thread's exit signal.
  bool AwaitRequest(std::uint64_t served) {
    std::unique_lock<std::mutex> lock(mutex_);
    cv_.wait(lock, [this, served] { return requested_ > served || quitting_; });
    return !quitting_;
  }

  // Called from wherever the engine presents. Releases `PumpFrame`.
  void PublishFrame() {
    {
      std::lock_guard<std::mutex> lock(mutex_);
      ++completed_;
    }
    cv_.notify_all();
  }

  // ------------------------------------------------------------------- lifecycle

  // Must be called before joining the driver thread. Without it the thread parks in
  // `AwaitRequest` forever and `retro_unload_game` deadlocks on the join.
  void Shutdown() {
    {
      std::lock_guard<std::mutex> lock(mutex_);
      quitting_ = true;
    }
    cv_.notify_all();
  }

  bool Quitting() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return quitting_;
  }

  std::uint64_t Completed() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return completed_;
  }

  std::uint64_t Requested() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return requested_;
  }

  // Test seam: reset between harness cases without reconstructing the engine.
  void Reset() {
    std::lock_guard<std::mutex> lock(mutex_);
    requested_ = 0;
    completed_ = 0;
    quitting_ = false;
  }

 private:
  mutable std::mutex mutex_;
  std::condition_variable cv_;
  // Monotonic, and never decremented. Their *difference* is the state, which is what
  // makes a timeout recoverable rather than desynchronising.
  std::uint64_t requested_ = 0;
  std::uint64_t completed_ = 0;
  bool quitting_ = false;
};

}  // namespace continuum

#endif  // CONTINUUM_FRAME_GATE_H
