/**
 * LibretroRuntime — instantiates a libretro core compiled to standalone wasm and
 * drives its C API.
 *
 * This is the frontend half of the contract in `core-shim/libretro_wasm_shim.c`. The
 * core module imports six `host.*` functions and a dozen WASI file syscalls; this
 * class supplies both, translates the libretro environment protocol, and exposes the
 * entry points the Rust bridge needs.
 *
 * ## What it deliberately does not do
 *
 * No rendering, no audio output, no input policy, no timing. Video and audio are
 * handed straight to `hooks`, and input is *asked for* through `hooks.inputState`.
 * In the app those hooks are wired to Rust; in `scripts/core-abi-test.mjs` they are
 * wired to plain arrays for headless testing. That is the whole reason the class has
 * no DOM dependency: the same file runs in Node and in the browser, so the ABI is
 * tested without a GPU.
 *
 * ## Memory model
 *
 * The core has its own linear memory, separate from the Rust module's. Nothing can
 * be shared by reference across that boundary, so per frame there is exactly one
 * copy: `video_refresh` hands the hook a view into core memory, and the hook copies
 * it wherever it needs to be. At 256x240 RGB565 that is 120 KB/frame — about 7 MB/s,
 * which is nothing next to the upload the GPU is about to do anyway.
 */

/** Values of `retro_pixel_format`. */
export const PIXEL_FORMAT = {
  ZERO_RGB1555: 0,
  XRGB8888: 1,
  RGB565: 2,
};

/** Environment command numbers used here (from libretro.h). */
const ENV = {
  SET_ROTATION: 1,
  GET_OVERSCAN: 2,
  GET_CAN_DUPE: 3,
  SET_MESSAGE: 6,
  SHUTDOWN: 7,
  SET_PERFORMANCE_LEVEL: 8,
  GET_SYSTEM_DIRECTORY: 9,
  SET_PIXEL_FORMAT: 10,
  SET_INPUT_DESCRIPTORS: 11,
  SET_KEYBOARD_CALLBACK: 12,
  SET_DISK_CONTROL_INTERFACE: 13,
  SET_HW_RENDER: 14,
  GET_VARIABLE: 15,
  SET_VARIABLES: 16,
  GET_VARIABLE_UPDATE: 17,
  SET_SUPPORT_NO_GAME: 18,
  GET_LIBRETRO_PATH: 19,
  SET_FRAME_TIME_CALLBACK: 21,
  SET_AUDIO_CALLBACK: 22,
  GET_RUMBLE_INTERFACE: 23,
  GET_INPUT_DEVICE_CAPABILITIES: 24,
  GET_LOG_INTERFACE: 27,
  GET_PERF_INTERFACE: 28,
  GET_CONTENT_DIRECTORY: 30,
  GET_SAVE_DIRECTORY: 31,
  SET_SYSTEM_AV_INFO: 32,
  SET_SUBSYSTEM_INFO: 34,
  SET_CONTROLLER_INFO: 35,
  SET_MEMORY_MAPS: 36 | 0x10000, // EXPERIMENTAL
  SET_GEOMETRY: 37,
  GET_USERNAME: 38,
  GET_LANGUAGE: 39,
  SET_AUDIO_BUFFER_STATUS_CALLBACK: 62,
  GET_AUDIO_VIDEO_ENABLE: 47 | 0x10000, // EXPERIMENTAL
  GET_INPUT_BITMASKS: 51 | 0x10000, // EXPERIMENTAL
  GET_CORE_OPTIONS_VERSION: 52,
  SET_CORE_OPTIONS: 53,
  SET_CORE_OPTIONS_INTL: 54,
  SET_CORE_OPTIONS_DISPLAY: 55,
  SET_SERIALIZATION_QUIRKS: 44,
  SET_MINIMUM_AUDIO_LATENCY: 63,
  GET_CURRENT_SOFTWARE_FRAMEBUFFER: 40 | 0x10000, // EXPERIMENTAL
  SET_SUPPORT_ACHIEVEMENTS: 42 | 0x10000, // EXPERIMENTAL
  SET_CONTENT_INFO_OVERRIDE: 65,
  GET_GAME_INFO_EXT: 66,
  GET_THROTTLE_STATE: 71 | 0x10000,
};

/** `sizeof(struct retro_system_content_info_override)` on wasm32: ptr + 2 bools, padded. */
const CONTENT_OVERRIDE_STRIDE = 8;

/**
 * Field offsets of `struct retro_game_info_ext` on wasm32 (7 pointers, a data
 * pointer, a size_t, two bools), and the allocation size including tail padding.
 */
const GAME_INFO_EXT = {
  FULL_PATH: 0,
  ARCHIVE_PATH: 4,
  ARCHIVE_FILE: 8,
  DIR: 12,
  NAME: 16,
  EXT: 20,
  META: 24,
  DATA: 28,
  SIZE: 32,
  FILE_IN_ARCHIVE: 36,
  PERSISTENT_DATA: 37,
  SIZEOF: 40,
};

/** WASI errnos we return. */
const ERRNO = { SUCCESS: 0, BADF: 8, NOENT: 44, NOSYS: 52 };

/** ABI version the shim must report; guards against a stale core build. */
const REQUIRED_SHIM_ABI = 1;

/**
 * Lifecycle counters for every core instantiated in this page.
 *
 * A multi-core emulator lives or dies on teardown: each core is a wasm module with
 * 16–32 MB of linear memory, so one leaked instance per system switch exhausts a phone
 * in a handful of swaps. "We call destroy()" is not evidence — the module is only
 * actually freed when the host garbage-collects it, which cannot happen while any
 * reference survives.
 *
 * So this tracks three separate things:
 *
 *   - `instantiated` / `destroyed` — did teardown run at all?
 *   - `collected` — did the module's memory actually get reclaimed? Reported by a
 *     `FinalizationRegistry`, i.e. by the collector itself rather than by our
 *     bookkeeping.
 *   - `liveBytes` — linear memory currently held by cores that have not been destroyed.
 *
 * `scripts/smoke-test.mjs` asserts on all of them after repeated NES↔GBA swaps.
 */
export const runtimeStats = {
  instantiated: 0,
  destroyed: 0,
  collected: 0,
  liveBytes: 0,
  peakLiveBytes: 0,
  /**
   * Most cores alive at any one moment. The multi-core memory policy's core claim is
   * that this never exceeds 1 — proving it needs a high-water mark, because a
   * momentary overlap during a switch would be invisible to a check made afterwards.
   */
  maxLive: 0,
  /** @type {string[]} ids of runtimes created but not yet destroyed */
  live: [],
};

/**
 * Observes actual collection of core modules. The callback fires only after the host
 * has genuinely reclaimed the object, which is the one signal that cannot be faked by
 * careful bookkeeping.
 */
const collectionWatcher =
  typeof FinalizationRegistry === 'undefined'
    ? null
    : new FinalizationRegistry((id) => {
        runtimeStats.collected++;
        console.debug(`[core] runtime '${id}' collected`);
      });

export class LibretroRuntime {
  /**
   * @param {object} hooks
   * @param {(frame: {data: Uint8Array, width: number, height: number, pitch: number, format: number}) => void} hooks.video
   *        Called from inside `run()`. `data` is a live view of core memory and is
   *        only valid for the duration of the call.
   * @param {(samples: Int16Array, frames: number) => void} hooks.audio
   *        Interleaved stereo i16, also a live view.
   * @param {(port: number, device: number, index: number, id: number) => number} hooks.inputState
   * @param {() => void} [hooks.inputPoll]
   * @param {(info: {pixelFormat?: number, geometry?: object, timing?: object}) => void} [hooks.systemChanged]
   * @param {(message: string) => void} [hooks.log]
   */
  constructor(hooks, id = 'core') {
    this.hooks = hooks;
    /** Identifier used in lifecycle logging and leak accounting. */
    this.id = id;
    this.destroyed = false;
    this.instance = null;
    this.exports = null;
    /** @type {WebAssembly.Memory|null} */
    this.memory = null;

    this.pixelFormat = PIXEL_FORMAT.ZERO_RGB1555; // libretro's default
    this.systemInfo = null;
    this.avInfo = null;
    this.gameLoaded = false;
    /**
     * Per-extension overrides of `need_fullpath`, from SET_CONTENT_INFO_OVERRIDE.
     *
     * Modern cores declare `need_fullpath = true` globally — fceumm does — and then
     * override it per extension. Without honouring this, a perfectly loadable ROM
     * looks like it needs a filesystem the browser cannot give it.
     *
     * @type {{extensions: string[], needFullpath: boolean, persistentData: boolean}[]}
     */
    this.contentOverrides = [];

    /** Allocation holding the ROM; must outlive `retro_load_game`. */
    this._romPtr = 0;
    this._romSize = 0;
    /** Every allocation made for the current game, freed together on unload. */
    this._loadAllocations = [];
    /** `retro_game_info_ext`, valid only during a load call. */
    this._gameInfoExtPtr = 0;
    /** Scratch allocation reused by `shim_av_info` / `shim_system_info`. */
    this._scratchPtr = 0;
    /** Cached typed-array views, rebuilt when the core's memory grows. */
    this._u8 = null;
    this._i16 = null;
    this._u32 = null;
    this._f64 = null;
    /** Cached views, keyed by pointer+length: cores reuse one framebuffer/audio buffer. */
    this._videoView = null;
    this._videoViewKey = '';
    this._audioView = null;
    this._audioViewKey = '';
  }

  // ------------------------------------------------------------ instantiation

  /**
   * @param {BufferSource} moduleBytes
   * @param {object} hooks see the constructor
   * @returns {Promise<LibretroRuntime>}
   */
  static async instantiate(moduleBytes, hooks, id = 'core') {
    const runtime = new LibretroRuntime(hooks, id);
    const imports = {
      host: runtime._hostImports(),
      wasi_snapshot_preview1: runtime._wasiImports(),
    };

    // `instantiate` accepts bytes or a Module; the caller decides whether to
    // compile-and-cache separately.
    const result =
      moduleBytes instanceof WebAssembly.Module
        ? { instance: await WebAssembly.instantiate(moduleBytes, imports), module: moduleBytes }
        : await WebAssembly.instantiate(moduleBytes, imports);

    runtime.instance = result.instance;
    runtime.exports = result.instance.exports;
    runtime.memory = runtime.exports.memory;
    runtime._refreshViews();

    const required = ['_initialize', 'shim_install', 'shim_abi_version', 'retro_init', 'retro_run', 'malloc'];
    for (const name of required) {
      if (typeof runtime.exports[name] !== 'function') {
        throw new Error(`core module is missing export '${name}'; was it built with core-shim?`);
      }
    }

    // Reactor module: run the wasi-libc initialisers before anything else.
    runtime.exports._initialize();

    const abi = runtime.exports.shim_abi_version();
    if (abi !== REQUIRED_SHIM_ABI) {
      throw new Error(`core shim ABI ${abi}, expected ${REQUIRED_SHIM_ABI}; rebuild the core`);
    }

    // Callbacks must be registered before `retro_init`, because cores query the
    // environment during `retro_set_environment`.
    runtime.exports.shim_install();
    runtime.exports.retro_init();

    runtime.systemInfo = runtime._readSystemInfo();

    // Leak accounting. `collectionWatcher` reports the module's *actual* collection
    // later, once nothing references it.
    runtime._bytes = runtime.memory.buffer.byteLength;
    runtimeStats.instantiated++;
    runtimeStats.liveBytes += runtime._bytes;
    runtimeStats.peakLiveBytes = Math.max(runtimeStats.peakLiveBytes, runtimeStats.liveBytes);
    runtimeStats.live.push(id);
    runtimeStats.maxLive = Math.max(runtimeStats.maxLive, runtimeStats.live.length);
    collectionWatcher?.register(runtime, id);
    console.info(
      `[core] '${id}' instantiated: ${runtime.systemInfo.name} ${runtime.systemInfo.version}, ` +
        `${(runtime._bytes / 1024 / 1024).toFixed(1)} MB linear memory`,
    );

    return runtime;
  }

  /**
   * Linear memory the core currently occupies.
   *
   * Measured live rather than at instantiation: a core grows its heap as it runs (mGBA
   * allocates ROM and save buffers on load), so the figure taken at startup understates
   * the real footprint by an order of magnitude.
   */
  get memoryBytes() {
    return this.memory ? this.memory.buffer.byteLength : 0;
  }

  /** Rebuilds cached views. Required after any core memory growth. */
  _refreshViews() {
    const buffer = this.memory.buffer;
    this._u8 = new Uint8Array(buffer);
    this._i16 = new Int16Array(buffer);
    this._u32 = new Uint32Array(buffer);
    this._f64 = new Float64Array(buffer);
    this._videoView = null;
    this._videoViewKey = '';
    this._audioView = null;
    this._audioViewKey = '';
  }

  _viewsStale() {
    return this._u8 === null || this._u8.buffer !== this.memory.buffer;
  }

  _u8view() {
    if (this._viewsStale()) this._refreshViews();
    return this._u8;
  }

  /** Reads a NUL-terminated C string from core memory. */
  _cstr(ptr) {
    if (!ptr) return '';
    const u8 = this._u8view();
    let end = ptr;
    while (end < u8.length && u8[end] !== 0) end++;
    return new TextDecoder().decode(u8.subarray(ptr, end));
  }

  /** A persistent 64-byte scratch allocation for the flattened shim structs. */
  _scratch() {
    if (!this._scratchPtr) {
      this._scratchPtr = this.exports.malloc(64);
      if (!this._scratchPtr) throw new Error('core malloc failed for scratch buffer');
    }
    return this._scratchPtr;
  }

  // ----------------------------------------------------------- host callbacks

  _hostImports() {
    return {
      video_refresh: (dataPtr, width, height, pitch) => {
        // A null pointer means "duplicate the previous frame", which is legal and
        // common when a core has nothing new to show.
        if (!dataPtr || !width || !height) return;
        if (this._viewsStale()) this._refreshViews();

        const length = pitch * height;
        const key = `${dataPtr}:${length}`;
        if (key !== this._videoViewKey) {
          this._videoView = new Uint8Array(this.memory.buffer, dataPtr, length);
          this._videoViewKey = key;
        }
        this.hooks.video({
          data: this._videoView,
          width,
          height,
          pitch,
          format: this.pixelFormat,
        });
      },

      audio_batch: (dataPtr, frames) => {
        if (!dataPtr || !frames) return frames;
        if (this._viewsStale()) this._refreshViews();
        // Cached like the video view: a core reuses one audio buffer, so the pointer
        // and length repeat every frame and a fresh view would be pure garbage.
        const samples = frames * 2;
        const key = `${dataPtr}:${samples}`;
        if (key !== this._audioViewKey) {
          this._audioView = new Int16Array(this.memory.buffer, dataPtr, samples);
          this._audioViewKey = key;
        }
        this.hooks.audio(this._audioView, frames);
        return frames;
      },

      input_poll: () => {
        this.hooks.inputPoll?.();
      },

      input_state: (port, device, index, id) => this.hooks.inputState(port, device, index, id),

      environment: (cmd, dataPtr) => (this._environment(cmd, dataPtr) ? 1 : 0),
    };
  }

  /**
   * The libretro environment protocol.
   *
   * Returning false means "unsupported", which cores are required to handle — so the
   * safe default for anything not listed is false. Only the commands that change how
   * the frontend must behave are actually implemented.
   */
  _environment(cmd, ptr) {
    if (this._viewsStale()) this._refreshViews();
    const u32 = this._u32;
    const u8 = this._u8;

    switch (cmd) {
      case ENV.SET_PIXEL_FORMAT: {
        const format = u32[ptr >> 2];
        if (format !== PIXEL_FORMAT.RGB565 && format !== PIXEL_FORMAT.XRGB8888) {
          // 0RGB1555 is legal libretro but obsolete; refusing keeps the renderer's
          // conversion paths to the two formats that matter.
          return false;
        }
        this.pixelFormat = format;
        this.hooks.systemChanged?.({ pixelFormat: format });
        return true;
      }

      case ENV.GET_CAN_DUPE:
        // The renderer re-presents its existing texture when a frame is duped.
        u8[ptr] = 1;
        return true;

      case ENV.SET_SYSTEM_AV_INFO: {
        const info = this._readAvInfoAt(ptr);
        this.avInfo = info;
        this.hooks.systemChanged?.({ geometry: info.geometry, timing: info.timing });
        return true;
      }

      case ENV.SET_GEOMETRY: {
        const geometry = this._readGeometryAt(ptr);
        if (this.avInfo) this.avInfo.geometry = geometry;
        this.hooks.systemChanged?.({ geometry });
        return true;
      }

      case ENV.SET_CONTENT_INFO_OVERRIDE: {
        // Array of { const char *extensions; bool need_fullpath; bool persistent_data; }
        // terminated by a NULL `extensions`.
        this.contentOverrides = [];
        if (!ptr) return true; // A null pointer means "no overrides", still supported.
        for (let entry = ptr; ; entry += CONTENT_OVERRIDE_STRIDE) {
          const extensionsPtr = u32[entry >> 2];
          if (!extensionsPtr) break;
          this.contentOverrides.push({
            extensions: this._cstr(extensionsPtr).split('|').filter(Boolean),
            needFullpath: u8[entry + 4] !== 0,
            persistentData: u8[entry + 5] !== 0,
          });
          // Defensive: a malformed, unterminated array must not spin forever.
          if (this.contentOverrides.length > 64) break;
        }
        return true;
      }

      case ENV.GET_GAME_INFO_EXT: {
        // Queried from inside `retro_load_game`. Cores that declared a
        // `need_fullpath` override use this to discover where the content actually
        // lives — refusing it makes them reject perfectly loadable content.
        if (!this._gameInfoExtPtr) return false;
        u32[ptr >> 2] = this._gameInfoExtPtr;
        return true;
      }

      case ENV.SET_ROTATION:
        // Rotation would need a shader change; refuse rather than silently ignore.
        return u32[ptr >> 2] === 0;

      case ENV.GET_VARIABLE:
        // No core options are set, so every variable is "unset" and the core keeps
        // its defaults. TODO(phase1c): back this with a settings UI.
        return false;

      case ENV.GET_VARIABLE_UPDATE:
        u8[ptr] = 0;
        return true;

      case ENV.GET_AUDIO_VIDEO_ENABLE:
        // Bit 0 = audio, bit 1 = video: both always enabled here.
        u32[ptr >> 2] = 0x3;
        return true;

      case ENV.GET_OVERSCAN:
        u8[ptr] = 0;
        return true;

      case ENV.SET_MESSAGE: {
        // struct { const char *msg; unsigned frames; }
        const text = this._cstr(u32[ptr >> 2]);
        this.hooks.log?.(`[core message] ${text}`);
        return true;
      }

      case ENV.SHUTDOWN:
        this.hooks.log?.('[core] requested shutdown');
        return true;

      // Accepted and ignored: metadata the frontend is free to disregard.
      case ENV.SET_PERFORMANCE_LEVEL:
      case ENV.SET_INPUT_DESCRIPTORS:
      case ENV.SET_CONTROLLER_INFO:
      case ENV.SET_MEMORY_MAPS:
      case ENV.SET_VARIABLES:
      case ENV.SET_CORE_OPTIONS:
      case ENV.SET_CORE_OPTIONS_INTL:
      case ENV.SET_CORE_OPTIONS_DISPLAY:
      case ENV.SET_SERIALIZATION_QUIRKS:
      case ENV.SET_SUPPORT_NO_GAME:
      case ENV.SET_SUBSYSTEM_INFO:
      case ENV.SET_SUPPORT_ACHIEVEMENTS:
        return true;

      // Refused, but named rather than left to fall through: fceumm asks for a
      // software framebuffer *every frame*, so an unnamed command here is 60 log lines
      // a second that bury real ones.
      //
      // GET_CURRENT_SOFTWARE_FRAMEBUFFER offers zero-copy rendering: the core would
      // draw straight into a buffer we own. Tempting, but the buffer it wants must
      // stay valid across `retro_run`, and ours lives in Rust behind a staging copy
      // that the renderer converts pixel formats through. Refusing means the core
      // uses its own framebuffer and hands it to `video_refresh`, which is the path
      // everything else here is built on.
      case ENV.GET_CURRENT_SOFTWARE_FRAMEBUFFER:
      // Audio latency is the ring buffer's business, and it is sized in Rust.
      case ENV.SET_MINIMUM_AUDIO_LATENCY:
        return false;

      // Refused deliberately, and not merely unimplemented: this one asks us to call
      // the core back whenever the audio buffer runs low, so that it can drop frames
      // to catch up. Returning true would promise a callback that never comes; the
      // core would then never adapt, having been told it did not need to. Frame
      // pacing is the Rust FramePacer's job anyway.
      case ENV.SET_AUDIO_BUFFER_STATUS_CALLBACK:
        return false;

      // Refused: unimplemented interfaces, and paths that do not exist in a browser.
      case ENV.GET_SYSTEM_DIRECTORY:
      case ENV.GET_SAVE_DIRECTORY:
      case ENV.GET_CONTENT_DIRECTORY:
      case ENV.GET_LIBRETRO_PATH:
      case ENV.GET_LOG_INTERFACE:
      case ENV.GET_PERF_INTERFACE:
      case ENV.GET_RUMBLE_INTERFACE:
      case ENV.GET_INPUT_DEVICE_CAPABILITIES:
      case ENV.GET_USERNAME:
      case ENV.GET_LANGUAGE:
      case ENV.GET_THROTTLE_STATE:
      case ENV.SET_KEYBOARD_CALLBACK:
      case ENV.SET_DISK_CONTROL_INTERFACE:
      case ENV.SET_FRAME_TIME_CALLBACK:
      case ENV.SET_AUDIO_CALLBACK:
        return false;

      case ENV.SET_HW_RENDER:
        // Hardware-rendered cores would need a GL context inside the core; this
        // project's cores must be software-rendered.
        return false;

      case ENV.GET_INPUT_BITMASKS:
        // Per-button queries only. Bitmasks are an optimisation, and refusing keeps
        // one input path instead of two.
        return false;

      case ENV.GET_CORE_OPTIONS_VERSION:
        u32[ptr >> 2] = 0;
        return true;

      default:
        this.hooks.log?.(`[core] unhandled environment command ${cmd}`);
        return false;
    }
  }

  // -------------------------------------------------------------- WASI stubs

  /**
   * Minimal WASI. The core only reaches for files when a frontend gives it paths,
   * and this one never does, so every call either logs (stdout/stderr) or fails
   * cleanly. Failing loudly-but-legally beats emulating a filesystem the core does
   * not need.
   */
  _wasiImports() {
    const notSupported = () => ERRNO.BADF;
    const noSuchFile = () => ERRNO.NOENT;
    return {
      // --- clocks: mgba reads the wall clock for the GBA's RTC ---
      clock_time_get: (_id, _precision, resultPtr) => {
        if (this._viewsStale()) this._refreshViews();
        // WASI wants nanoseconds in a 64-bit value; write it as two 32-bit halves to
        // avoid requiring BigInt support in the import signature.
        const nanos = BigInt(Math.round(Date.now() * 1e6));
        const u32 = this._u32;
        u32[resultPtr >> 2] = Number(nanos & 0xffffffffn);
        u32[(resultPtr >> 2) + 1] = Number((nanos >> 32n) & 0xffffffffn);
        return ERRNO.SUCCESS;
      },
      clock_res_get: (_id, resultPtr) => {
        if (this._viewsStale()) this._refreshViews();
        this._u32[resultPtr >> 2] = 1_000_000; // 1 ms, honestly reported
        this._u32[(resultPtr >> 2) + 1] = 0;
        return ERRNO.SUCCESS;
      },

      // --- environment: empty, but the query must succeed ---
      environ_sizes_get: (countPtr, sizePtr) => {
        if (this._viewsStale()) this._refreshViews();
        this._u32[countPtr >> 2] = 0;
        this._u32[sizePtr >> 2] = 0;
        return ERRNO.SUCCESS;
      },
      environ_get: () => ERRNO.SUCCESS,

      // --- directories: nothing exists, and nothing may be created ---
      fd_readdir: () => ERRNO.BADF,
      path_create_directory: noSuchFile,
      path_remove_directory: noSuchFile,
      path_unlink_file: noSuchFile,
      path_readlink: noSuchFile,
      path_rename: noSuchFile,
      random_get: (bufferPtr, length) => {
        if (this._viewsStale()) this._refreshViews();
        // Real entropy: a core seeding an RNG from this deserves better than zeros.
        crypto.getRandomValues(this._u8.subarray(bufferPtr, bufferPtr + length));
        return ERRNO.SUCCESS;
      },

      fd_write: (fd, iovsPtr, iovsLen, writtenPtr) => {
        if (this._viewsStale()) this._refreshViews();
        const u32 = this._u32;
        let total = 0;
        const chunks = [];
        for (let i = 0; i < iovsLen; i++) {
          const base = u32[(iovsPtr >> 2) + i * 2];
          const length = u32[(iovsPtr >> 2) + i * 2 + 1];
          if (length > 0) chunks.push(this._u8.subarray(base, base + length));
          total += length;
        }
        u32[writtenPtr >> 2] = total;
        if (chunks.length && this.hooks.log) {
          const text = chunks.map((c) => new TextDecoder().decode(c)).join('').trimEnd();
          if (text) this.hooks.log(`[core:${fd === 2 ? 'stderr' : 'stdout'}] ${text}`);
        }
        return ERRNO.SUCCESS;
      },
      fd_read: notSupported,
      fd_close: notSupported,
      fd_seek: notSupported,
      fd_tell: notSupported,
      fd_fdstat_get: notSupported,
      fd_fdstat_set_flags: notSupported,
      // Reporting no preopened directories stops wasi-libc's startup scan.
      fd_prestat_get: () => ERRNO.BADF,
      fd_prestat_dir_name: () => ERRNO.BADF,
      path_open: () => ERRNO.NOENT,
      path_filestat_get: () => ERRNO.NOENT,
      proc_exit: (code) => {
        throw new Error(`core called proc_exit(${code})`);
      },
    };
  }

  // ------------------------------------------------------------- libretro API

  _readSystemInfo() {
    const ptr = this._scratch();
    this.exports.shim_system_info(ptr);
    if (this._viewsStale()) this._refreshViews();
    const u32 = this._u32;
    const base = ptr >> 2;
    return {
      name: this._cstr(u32[base]),
      version: this._cstr(u32[base + 1]),
      validExtensions: this._cstr(u32[base + 2]).split('|').filter(Boolean),
      needFullpath: u32[base + 3] !== 0,
      blockExtract: u32[base + 4] !== 0,
    };
  }

  /** Reads `retro_system_av_info` from the flattened doubles the shim writes. */
  readAvInfo() {
    const ptr = this._scratch();
    this.exports.shim_av_info(ptr);
    if (this._viewsStale()) this._refreshViews();
    const f = this._f64;
    const base = ptr >> 3;
    this.avInfo = {
      geometry: {
        baseWidth: f[base],
        baseHeight: f[base + 1],
        maxWidth: f[base + 2],
        maxHeight: f[base + 3],
        aspectRatio: f[base + 4],
      },
      timing: { fps: f[base + 5], sampleRate: f[base + 6] },
    };
    return this.avInfo;
  }

  /**
   * A/V info flattened for the Rust bridge:
   * `[baseWidth, baseHeight, maxWidth, maxHeight, aspectRatio, fps, sampleRate]`.
   *
   * A flat `Float64Array` rather than the object, because the Rust side reads it
   * through a typed array without needing property lookups or a serde dependency.
   */
  avInfoArray() {
    const info = this.avInfo ?? this.readAvInfo();
    return Float64Array.of(
      info.geometry.baseWidth,
      info.geometry.baseHeight,
      info.geometry.maxWidth,
      info.geometry.maxHeight,
      info.geometry.aspectRatio,
      info.timing.fps,
      info.timing.sampleRate,
    );
  }

  /** Reads a C `retro_system_av_info` at `ptr` (used by SET_SYSTEM_AV_INFO). */
  _readAvInfoAt(ptr) {
    const u32 = this._u32;
    const f64 = this._f64;
    const geometry = this._readGeometryAt(ptr);
    // Layout: geometry{5 x u32/float = 20 bytes} then padding to 8, then 2 doubles.
    const timingBase = (ptr + 24) >> 3;
    return {
      geometry,
      timing: { fps: f64[timingBase], sampleRate: f64[timingBase + 1] },
    };
  }

  _readGeometryAt(ptr) {
    const u32 = this._u32;
    const f32 = new Float32Array(this.memory.buffer, ptr, 5);
    const base = ptr >> 2;
    return {
      baseWidth: u32[base],
      baseHeight: u32[base + 1],
      maxWidth: u32[base + 2],
      maxHeight: u32[base + 3],
      aspectRatio: f32[4],
    };
  }

  /**
   * Whether content with this extension must be loaded from a path rather than
   * memory. Overrides win over the core's global flag.
   *
   * @param {string} extension without the dot, e.g. `"nes"`
   */
  needsFullpath(extension) {
    const wanted = extension.replace(/^\./, '').toLowerCase();
    for (const override of this.contentOverrides) {
      if (override.extensions.some((e) => e.toLowerCase() === wanted)) {
        return override.needFullpath;
      }
    }
    return this.systemInfo?.needFullpath ?? false;
  }

  /**
   * Copies content into core memory and loads it.
   * @param {Uint8Array} rom
   * @param {string} [extension] used to resolve `need_fullpath` overrides
   */
  loadGame(rom, extension = '', name = 'content') {
    if (this.needsFullpath(extension)) {
      throw new Error(
        `core '${this.systemInfo?.name}' requires a file path for '${extension || 'this content'}', ` +
          'which a browser cannot provide',
      );
    }
    if (this.gameLoaded) this.unloadGame();

    const romPtr = this._alloc(rom.length);
    if (this._viewsStale()) this._refreshViews();
    this._u8.set(rom, romPtr);

    // Prepared *before* the load call: the core queries GET_GAME_INFO_EXT from
    // inside `retro_load_game`, so it has to already be answerable.
    this._gameInfoExtPtr = this._buildGameInfoExt({
      dataPtr: romPtr,
      size: rom.length,
      extension: extension.replace(/^\./, '').toLowerCase(),
      name,
    });

    let ok = false;
    try {
      ok = this.exports.shim_load_game(romPtr, rom.length) !== 0;
    } finally {
      // Valid only during the load call, per the libretro contract.
      this._gameInfoExtPtr = 0;
    }

    if (!ok) {
      this._freeLoadAllocations();
      throw new Error('retro_load_game rejected the content');
    }

    // Cores are allowed to keep `info.data` instead of copying it, so the ROM
    // allocation lives until unload.
    this._romPtr = romPtr;
    this._romSize = rom.length;
    this.gameLoaded = true;
    return this.readAvInfo();
  }

  /** Allocates in core memory and records it for cleanup on unload. */
  _alloc(bytes) {
    const ptr = this.exports.malloc(bytes);
    if (!ptr) throw new Error(`core malloc(${bytes}) failed`);
    this._loadAllocations.push(ptr);
    return ptr;
  }

  _allocCString(text) {
    const encoded = new TextEncoder().encode(text);
    const ptr = this._alloc(encoded.length + 1);
    if (this._viewsStale()) this._refreshViews();
    this._u8.set(encoded, ptr);
    this._u8[ptr + encoded.length] = 0;
    return ptr;
  }

  /** Builds a `retro_game_info_ext` in core memory. */
  _buildGameInfoExt({ dataPtr, size, extension, name }) {
    const extPtr = extension ? this._allocCString(extension) : 0;
    const namePtr = name ? this._allocCString(name) : 0;
    const ptr = this._alloc(GAME_INFO_EXT.SIZEOF);

    // A synthetic filename, not a real one. Cores are entitled to read the content's
    // *name* even when `need_fullpath` is false — several decide which system they are
    // emulating from the extension, and Genesis Plus GX is one: it copies `full_path`
    // into a buffer and then reads the last three characters to pick between Mega
    // Drive, Master System, Game Gear and SG-1000. Passing NULL there is what made a
    // Master System cart boot as a Mega Drive one, running Z80 code on the 68000.
    //
    // The path deliberately has no directory component, so a core that ignored
    // `need_fullpath` and tried to open it would fail immediately rather than reach
    // for something outside the sandbox.
    const fullPath = extension ? `${name || 'content'}.${extension}` : name || 'content';
    const fullPathPtr = this._allocCString(fullPath);
    // Empty string rather than NULL: cores `strncpy` this field unconditionally.
    const dirPtr = this._allocCString('');

    if (this._viewsStale()) this._refreshViews();
    this._u8.fill(0, ptr, ptr + GAME_INFO_EXT.SIZEOF);
    const u32 = this._u32;
    u32[(ptr + GAME_INFO_EXT.FULL_PATH) >> 2] = fullPathPtr;
    u32[(ptr + GAME_INFO_EXT.DIR) >> 2] = dirPtr;
    // archive_path / archive_file stay NULL: `file_in_archive` is false, which makes
    // them meaningless by contract. `meta` is explicitly allowed to be NULL.
    u32[(ptr + GAME_INFO_EXT.NAME) >> 2] = namePtr;
    u32[(ptr + GAME_INFO_EXT.EXT) >> 2] = extPtr;
    u32[(ptr + GAME_INFO_EXT.DATA) >> 2] = dataPtr;
    u32[(ptr + GAME_INFO_EXT.SIZE) >> 2] = size;
    this._u8[ptr + GAME_INFO_EXT.FILE_IN_ARCHIVE] = 0;
    // True: the buffer outlives the load call, so the core may reference it rather
    // than copy.
    this._u8[ptr + GAME_INFO_EXT.PERSISTENT_DATA] = 1;
    return ptr;
  }

  _freeLoadAllocations() {
    for (const ptr of this._loadAllocations) this.exports.free(ptr);
    this._loadAllocations = [];
    this._romPtr = 0;
    this._romSize = 0;
    this._gameInfoExtPtr = 0;
  }

  run() {
    this.exports.retro_run();
  }

  reset() {
    this.exports.retro_reset();
  }

  unloadGame() {
    if (!this.gameLoaded) return;
    this.exports.retro_unload_game();
    this._freeLoadAllocations();
    this.gameLoaded = false;
  }

  serializeSize() {
    return this.exports.retro_serialize_size();
  }

  /** @returns {Uint8Array} a copy of the state, owned by the caller. */
  serialize() {
    const size = this.serializeSize();
    if (!size) return new Uint8Array(0);
    const ptr = this.exports.malloc(size);
    if (!ptr) throw new Error(`core malloc(${size}) failed for save state`);
    try {
      if (this.exports.retro_serialize(ptr, size) === 0) {
        throw new Error('retro_serialize failed');
      }
      if (this._viewsStale()) this._refreshViews();
      return this._u8.slice(ptr, ptr + size);
    } finally {
      this.exports.free(ptr);
    }
  }

  /** @param {Uint8Array} state */
  unserialize(state) {
    const ptr = this.exports.malloc(state.length);
    if (!ptr) throw new Error(`core malloc(${state.length}) failed for load state`);
    try {
      if (this._viewsStale()) this._refreshViews();
      this._u8.set(state, ptr);
      if (this.exports.retro_unserialize(ptr, state.length) === 0) {
        throw new Error('retro_unserialize rejected the state');
      }
    } finally {
      this.exports.free(ptr);
    }
  }

  /**
   * Tears the core down and drops every reference to its module.
   *
   * Two distinct jobs, and both matter:
   *
   * 1. Tell the core to clean up — `retro_unload_game`, `retro_deinit`, free our own
   *    allocations inside its heap. Skipping this leaks the core's *internal* state
   *    (save RAM, audio buffers) and can leave hardware-ish state initialised.
   * 2. Release every JS reference into the module: exports, memory, cached typed-array
   *    views. This is what actually allows the host to reclaim 16–32 MB of linear
   *    memory. A single retained `Uint8Array` view keeps the whole `ArrayBuffer` — and
   *    therefore the whole core — alive, which is exactly how a swap-per-session leak
   *    happens.
   *
   * Idempotent, and never throws: it runs during teardown, where a second failure would
   * mask the first.
   */
  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;

    try {
      this.unloadGame();
      this.exports?.retro_deinit?.();
      if (this._scratchPtr) {
        this.exports.free(this._scratchPtr);
        this._scratchPtr = 0;
      }
    } catch (err) {
      this.hooks.log?.(`[core] error during teardown: ${err}`);
    }

    // Account for the size the core actually reached, not the size it started at.
    const finalBytes = this.memoryBytes || this._bytes || 0;
    runtimeStats.peakLiveBytes = Math.max(
      runtimeStats.peakLiveBytes,
      runtimeStats.liveBytes - (this._bytes ?? 0) + finalBytes,
    );
    runtimeStats.destroyed++;
    runtimeStats.liveBytes = Math.max(0, runtimeStats.liveBytes - (this._bytes ?? 0));
    const index = runtimeStats.live.indexOf(this.id);
    if (index >= 0) runtimeStats.live.splice(index, 1);
    console.info(
      `[core] '${this.id}' destroyed; ${runtimeStats.live.length} runtime(s) still live`,
    );

    // Drop everything pointing into the module's memory. Any of these left behind would
    // pin the entire linear memory.
    this._u8 = null;
    this._i16 = null;
    this._u32 = null;
    this._f64 = null;
    this._videoView = null;
    this._videoViewKey = '';
    this._audioView = null;
    this._audioViewKey = '';
    this.exports = null;
    this.instance = null;
    this.memory = null;
    // Hooks close over the host's staging buffers; releasing them breaks the last
    // reference cycle between the core and the frontend.
    this.hooks = { video() {}, audio() {}, inputState: () => 0 };
  }
}
