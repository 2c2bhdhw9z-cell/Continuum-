// Telemetry: what a sideloaded build has instead of Xcode (owner, 25 October 2026: "PUT the
// telemetry to GET what you need, because the logs don't do shit").
//
// Three things, all written straight to files with write(2), so they are on disk the moment they
// happen and survive the app dying a microsecond later:
//
// - core-log.txt: EVERY line every emulator sends through the libretro log interface (until now
//   only cBoy's went anywhere but os_log), plus `app:` notes from the engine (JIT memory handed
//   out, per-second frame and sound figures).
// - crash.txt: a crash catcher for SIGSEGV, SIGBUS, SIGILL, SIGTRAP, SIGABRT and SIGFPE: the
//   signal, the faulting address, the registers, and a frame-pointer backtrace with each address
//   named as <library>+<offset> (symbol), then the crash carries on as before.
// - The watchdog: a thread that watches the game's frame. A frame that takes over 50 ms is
//   sampled once (where the game thread is right then, and its callers), so a hitch says where it
//   was spent; a frame stuck for 3 s is sampled every 2 s, so a freeze says where it is stuck.
//
// The previous two sessions' files are kept (-previous, -older), like the activity log.

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#if defined(__APPLE__) && defined(__aarch64__)
#include <TargetConditionals.h>
#endif

#if defined(__APPLE__)

#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <mach/mach.h>
#include <mach/mach_time.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#if TARGET_OS_IPHONE
#include <os/proc.h>
#endif

static int g_log_fd = -1;
static int g_crash_fd = -1;
static char g_dir[1024];
static uint64_t g_t0;
static mach_timebase_info_data_t g_tb;
static pthread_mutex_t g_log_lock = PTHREAD_MUTEX_INITIALIZER;
static size_t g_log_bytes;
// A core that logs thousands of lines a second must not slow the game: past this many lines in
// one second the rest are counted, and the count is written instead.
#define LINES_PER_SECOND 400
static uint64_t g_line_second;
static unsigned g_lines_this_second;
static unsigned g_lines_dropped;
#define LOG_LIMIT (6u << 20)

static uint64_t now_ns(void) {
    uint64_t t = mach_absolute_time();
    return t * g_tb.numer / g_tb.denom;
}

// ------------------------------------------------------------------ async-signal-safe writing

static void put(int fd, const char *s) {
    if (fd < 0 || s == NULL) return;
    size_t n = strlen(s);
    while (n > 0) {
        ssize_t w = write(fd, s, n);
        if (w <= 0) {
            if (w < 0 && errno == EINTR) continue;
            return;
        }
        s += w;
        n -= (size_t)w;
    }
}

static void put_hex(int fd, uint64_t v) {
    char b[19];
    b[0] = '0';
    b[1] = 'x';
    int i = 18;
    b[i] = 0;
    if (v == 0) {
        b[--i] = '0';
    }
    while (v && i > 2) {
        b[--i] = "0123456789abcdef"[v & 15];
        v >>= 4;
    }
    int k = 2;
    while (i < 18) b[k++] = b[i++];
    b[k] = 0;
    put(fd, b);
}

static void put_dec(int fd, uint64_t v) {
    char b[24];
    int i = 23;
    b[i] = 0;
    do {
        b[--i] = (char)('0' + v % 10);
        v /= 10;
    } while (v && i > 0);
    put(fd, b + i);
}

static const char *base_name(const char *path) {
    if (path == NULL) return "?";
    const char *slash = strrchr(path, '/');
    return slash ? slash + 1 : path;
}

// <library>+<offset> (symbol+offset). dladdr is not on the async-signal-safe list; it is what every
// iOS crash reporter uses anyway, and the raw address is written before it is called.
static void put_where(int fd, uint64_t pc) {
    put_hex(fd, pc);
    Dl_info info;
    if (pc != 0 && dladdr((const void *)pc, &info) && info.dli_fbase != NULL) {
        put(fd, " ");
        put(fd, base_name(info.dli_fname));
        put(fd, "+");
        put_hex(fd, pc - (uint64_t)info.dli_fbase);
        if (info.dli_sname != NULL && info.dli_saddr != NULL) {
            put(fd, " (");
            put(fd, info.dli_sname);
            put(fd, "+");
            put_hex(fd, pc - (uint64_t)info.dli_saddr);
            put(fd, ")");
        }
    }
}

static void put_time(int fd) {
    uint64_t ms = (now_ns() - g_t0) / 1000000;
    put(fd, "[");
    put_dec(fd, ms / 1000);
    put(fd, ".");
    char frac[4] = {(char)('0' + ms % 1000 / 100), (char)('0' + ms % 100 / 10), (char)('0' + ms % 10), 0};
    put(fd, frac);
    put(fd, "] ");
}

// ------------------------------------------------------------------------------ the log file

static void rotate(const char *name) {
    char cur[1200], prev[1200], older[1200];
    snprintf(cur, sizeof cur, "%s/%s.txt", g_dir, name);
    snprintf(prev, sizeof prev, "%s/%s-previous.txt", g_dir, name);
    snprintf(older, sizeof older, "%s/%s-older.txt", g_dir, name);
    struct stat st;
    if (stat(cur, &st) == 0) {
        if (stat(prev, &st) == 0) rename(prev, older);
        rename(cur, prev);
    }
}

static void log_line(const char *tag, const char *text) {
    if (g_log_fd < 0 || text == NULL) return;
    pthread_mutex_lock(&g_log_lock);
    uint64_t second = (now_ns() - g_t0) / 1000000000ull;
    if (second != g_line_second) {
        if (g_lines_dropped) {
            put_time(g_log_fd);
            put(g_log_fd, "app: ");
            put_dec(g_log_fd, g_lines_dropped);
            put(g_log_fd, " more core lines in that second were not written\n");
        }
        g_line_second = second;
        g_lines_this_second = 0;
        g_lines_dropped = 0;
    }
    if (++g_lines_this_second > LINES_PER_SECOND) {
        g_lines_dropped++;
        pthread_mutex_unlock(&g_log_lock);
        return;
    }
    if (g_log_bytes > LOG_LIMIT) {
        // Keep the file to a size an upload can carry: start it again, saying so.
        ftruncate(g_log_fd, 0);
        lseek(g_log_fd, 0, SEEK_SET);
        g_log_bytes = 0;
        put(g_log_fd, "(the log reached 6 MB and was started again)\n");
    }
    char line[2300];
    uint64_t ms = (now_ns() - g_t0) / 1000000;
    int n = snprintf(line, sizeof line, "[%llu.%03llu] %s%s\n", (unsigned long long)(ms / 1000),
                     (unsigned long long)(ms % 1000), tag, text);
    if (n > 0) {
        if ((size_t)n >= sizeof line) n = sizeof line - 1;
        put(g_log_fd, line);
        g_log_bytes += (size_t)n;
    }
    pthread_mutex_unlock(&g_log_lock);
}

void continuum_tel_core_log(int level, const char *text) {
    static const char *tags[] = {"core DEBUG: ", "core: ", "core WARN: ", "core ERROR: "};
    log_line(tags[level < 0 ? 0 : level > 3 ? 3 : level], text);
}

void continuum_tel_note(const char *text) { log_line("app: ", text); }

// ------------------------------------------------------------------------------ the watchdog

static volatile uint64_t g_frame_start;   // 0 while no frame runs
static volatile uint64_t g_frame_number;
static mach_port_t g_game_thread = MACH_PORT_NULL;
static pthread_t g_game_pthread;
static unsigned g_slow_samples;
#define SLOW_NS (50ull * 1000000ull)
#define STUCK_NS (3000ull * 1000000ull)
#define MAX_SLOW_SAMPLES 400

void continuum_tel_frame_begin(void) {
    pthread_t self = pthread_self();
    if (g_game_thread == MACH_PORT_NULL || !pthread_equal(self, g_game_pthread)) {
        mach_port_t t = mach_thread_self();
        mach_port_t old = g_game_thread;
        g_game_pthread = self;
        g_game_thread = t;
        if (old != MACH_PORT_NULL) mach_port_deallocate(mach_task_self(), old);
    }
    g_frame_number++;
    __atomic_store_n(&g_frame_start, now_ns(), __ATOMIC_RELEASE);
}

void continuum_tel_frame_end(void) { __atomic_store_n(&g_frame_start, 0, __ATOMIC_RELEASE); }

// Reads one word of the game thread's stack without trusting the pointer.
static bool read_word(uint64_t addr, uint64_t *out) {
    if (addr == 0 || (addr & 7) != 0) return false;
    vm_size_t got = 0;
    return vm_read_overwrite(mach_task_self(), (vm_address_t)addr, 8, (vm_address_t)out, &got) == KERN_SUCCESS &&
           got == 8;
}

#define SAMPLE_FRAMES 24

static void sample_game_thread(const char *what, uint64_t frame, uint64_t elapsed_ns) {
    mach_port_t t = g_game_thread;
    if (t == MACH_PORT_NULL) return;
    uint64_t pcs[SAMPLE_FRAMES];
    int count = 0;
    if (thread_suspend(t) != KERN_SUCCESS) return;
    arm_thread_state64_t st;
    mach_msg_type_number_t n = ARM_THREAD_STATE64_COUNT;
    if (thread_get_state(t, ARM_THREAD_STATE64, (thread_state_t)&st, &n) == KERN_SUCCESS) {
        pcs[count++] = (uint64_t)arm_thread_state64_get_pc(st);
        pcs[count++] = (uint64_t)arm_thread_state64_get_lr(st);
        uint64_t fp = (uint64_t)arm_thread_state64_get_fp(st);
        while (count < SAMPLE_FRAMES && fp != 0) {
            uint64_t next = 0, ret = 0;
            if (!read_word(fp, &next) || !read_word(fp + 8, &ret)) break;
            if (ret == 0) break;
            pcs[count++] = ret;
            if (next <= fp) break;
            fp = next;
        }
    }
    thread_resume(t);
    // Named only after the thread runs again: dladdr takes dyld's lock, which the suspended thread
    // might have been holding.
    pthread_mutex_lock(&g_log_lock);
    put_time(g_log_fd);
    put(g_log_fd, "app: ");
    put(g_log_fd, what);
    put(g_log_fd, " frame ");
    put_dec(g_log_fd, frame);
    put(g_log_fd, " at ");
    put_dec(g_log_fd, elapsed_ns / 1000000);
    put(g_log_fd, " ms:\n");
    for (int i = 0; i < count; i++) {
        put(g_log_fd, i == 0 ? "    pc " : i == 1 ? "    lr " : "    <- ");
        put_where(g_log_fd, pcs[i] & 0x0000FFFFFFFFFFFFull);
        put(g_log_fd, "\n");
    }
    pthread_mutex_unlock(&g_log_lock);
}

// ------------------------------------------------------- profiler, threads and memory

// The game thread's program counter, sampled every 10 ms while a frame runs: a statistical
// profile of where the frame's time goes, reported every 5 seconds by library and offset (the
// offsets are named off the phone from the same build's symbol tables).
#define PROF_MAX 1024
static uint64_t g_prof[PROF_MAX];
static unsigned g_prof_n;
static uint64_t g_prof_since;

static void prof_sample(void) {
    mach_port_t t = g_game_thread;
    if (t == MACH_PORT_NULL || g_prof_n >= PROF_MAX) return;
    if (thread_suspend(t) != KERN_SUCCESS) return;
    arm_thread_state64_t st;
    mach_msg_type_number_t n = ARM_THREAD_STATE64_COUNT;
    kern_return_t r = thread_get_state(t, ARM_THREAD_STATE64, (thread_state_t)&st, &n);
    thread_resume(t);
    if (r == KERN_SUCCESS) g_prof[g_prof_n++] = (uint64_t)arm_thread_state64_get_pc(st) & 0x0000FFFFFFFFFFFFull;
}

typedef struct {
    const void *base;
    const char *name;
    unsigned count;
    uint64_t off[64];
    unsigned off_n[64];
    unsigned offs;
} ProfImage;

static void prof_report(void) {
    if (g_prof_n == 0) return;
    static ProfImage img[24];
    unsigned images = 0, jit = 0;
    memset(img, 0, sizeof img);
    for (unsigned i = 0; i < g_prof_n; i++) {
        Dl_info info;
        if (!dladdr((const void *)g_prof[i], &info) || info.dli_fbase == NULL) {
            jit++;
            continue;
        }
        unsigned k = 0;
        while (k < images && img[k].base != info.dli_fbase) k++;
        if (k == images) {
            if (images == 24) continue;
            images++;
            img[k].base = info.dli_fbase;
            img[k].name = base_name(info.dli_fname);
        }
        img[k].count++;
        uint64_t off = (g_prof[i] - (uint64_t)info.dli_fbase) & ~0x3Full;
        unsigned j = 0;
        while (j < img[k].offs && img[k].off[j] != off) j++;
        if (j == img[k].offs) {
            if (j == 64) continue;
            img[k].offs++;
            img[k].off[j] = off;
        }
        img[k].off_n[j]++;
    }
    char line[2048];
    int len = snprintf(line, sizeof line, "profile of the game thread over %llu s (%u samples, one per 10 ms inside frames):",
                       (unsigned long long)((now_ns() - g_prof_since) / 1000000000ull), g_prof_n);
    if (jit) len += snprintf(line + len, sizeof line - len, " generated code (JIT) %u%%;", 100 * jit / g_prof_n);
    for (unsigned pass = 0; pass < images && len < (int)sizeof line - 200; pass++) {
        unsigned best = 0;
        for (unsigned k = 1; k < images; k++) if (img[k].count > img[best].count) best = k;
        if (img[best].count == 0) break;
        ProfImage *m = &img[best];
        len += snprintf(line + len, sizeof line - len, " %s %u%% [", m->name, 100 * m->count / g_prof_n);
        for (unsigned shown = 0; shown < 8 && len < (int)sizeof line - 40; shown++) {
            unsigned bj = 0;
            for (unsigned j = 1; j < m->offs; j++) if (m->off_n[j] > m->off_n[bj]) bj = j;
            if (m->off_n[bj] == 0) break;
            len += snprintf(line + len, sizeof line - len, "%s+0x%llx %u", shown ? ", " : "", (unsigned long long)m->off[bj], m->off_n[bj]);
            m->off_n[bj] = 0;
        }
        len += snprintf(line + len, sizeof line - len, "];");
        m->count = 0;
    }
    log_line("app: ", line);
    g_prof_n = 0;
}

// Per-thread processor time over the last second, by thread name, plus memory.
#define THREADS_MAX 160
static uint64_t g_thread_id[THREADS_MAX];
static uint64_t g_thread_us[THREADS_MAX];
static unsigned g_threads;
static uint64_t g_threads_at;

static void threads_report(void) {
    thread_act_array_t list;
    mach_msg_type_number_t count = 0;
    if (task_threads(mach_task_self(), &list, &count) != KERN_SUCCESS) return;
    uint64_t now = now_ns();
    double secs = g_threads_at ? (double)(now - g_threads_at) / 1e9 : 0;
    struct { char name[48]; double pct; } top[THREADS_MAX];
    unsigned tops = 0;
    double total = 0;
    uint64_t new_id[THREADS_MAX], new_us[THREADS_MAX];
    unsigned news = 0;
    for (mach_msg_type_number_t i = 0; i < count; i++) {
        thread_identifier_info_data_t idi;
        mach_msg_type_number_t n1 = THREAD_IDENTIFIER_INFO_COUNT;
        thread_basic_info_data_t bi;
        mach_msg_type_number_t n2 = THREAD_BASIC_INFO_COUNT;
        if (thread_info(list[i], THREAD_IDENTIFIER_INFO, (thread_info_t)&idi, &n1) == KERN_SUCCESS &&
            thread_info(list[i], THREAD_BASIC_INFO, (thread_info_t)&bi, &n2) == KERN_SUCCESS) {
            uint64_t us = (uint64_t)bi.user_time.seconds * 1000000 + bi.user_time.microseconds +
                          (uint64_t)bi.system_time.seconds * 1000000 + bi.system_time.microseconds;
            if (news < THREADS_MAX) {
                new_id[news] = idi.thread_id;
                new_us[news++] = us;
            }
            uint64_t before = us;
            for (unsigned k = 0; k < g_threads; k++) if (g_thread_id[k] == idi.thread_id) { before = g_thread_us[k]; break; }
            if (secs > 0 && us > before && tops < THREADS_MAX) {
                double pct = 100.0 * (double)(us - before) / 1e6 / secs;
                total += pct;
                top[tops].name[0] = 0;
                pthread_t pt = pthread_from_mach_thread_np(list[i]);
                if (pt) pthread_getname_np(pt, top[tops].name, sizeof top[tops].name);
                if (!top[tops].name[0]) {
                    if (pthread_equal(pt, g_game_pthread)) snprintf(top[tops].name, sizeof top[tops].name, "game (main)");
                    else snprintf(top[tops].name, sizeof top[tops].name, "thread %llu", (unsigned long long)idi.thread_id);
                } else if (pt && pthread_equal(pt, g_game_pthread)) {
                    strncat(top[tops].name, " (game)", sizeof top[tops].name - strlen(top[tops].name) - 1);
                }
                top[tops++].pct = pct;
            }
        }
        mach_port_deallocate(mach_task_self(), list[i]);
    }
    vm_deallocate(mach_task_self(), (vm_address_t)list, count * sizeof(thread_act_t));
    memcpy(g_thread_id, new_id, news * sizeof new_id[0]);
    memcpy(g_thread_us, new_us, news * sizeof new_us[0]);
    g_threads = news;
    g_threads_at = now;
    if (secs <= 0) return;

    task_vm_info_data_t vm;
    mach_msg_type_number_t vn = TASK_VM_INFO_COUNT;
    uint64_t footprint = 0;
    if (task_info(mach_task_self(), TASK_VM_INFO, (task_info_t)&vm, &vn) == KERN_SUCCESS) footprint = vm.phys_footprint;
    uint64_t avail = 0;
#if TARGET_OS_IPHONE
    avail = os_proc_available_memory();
#endif
    char line[2048];
    int len = snprintf(line, sizeof line, "threads: %u, %.0f%% of one core in all; memory %llu MB used, %llu MB more allowed;",
                       count, total, (unsigned long long)(footprint >> 20), (unsigned long long)(avail >> 20));
    for (unsigned shown = 0; shown < 10 && len < (int)sizeof line - 80; shown++) {
        unsigned b = 0;
        for (unsigned k = 1; k < tops; k++) if (top[k].pct > top[b].pct) b = k;
        if (tops == 0 || top[b].pct < 0.5) break;
        len += snprintf(line + len, sizeof line - len, " %s %.0f%%,", top[b].name, top[b].pct);
        top[b].pct = -1;
    }
    log_line("app: ", line);
}

static void *watchdog(void *arg) {
    (void)arg;
    pthread_setname_np("Continuum watchdog");
    uint64_t sampled_frame = 0, last_stuck_sample = 0;
    uint64_t second = 0;
    unsigned this_second = 0;
    for (;;) {
        usleep(10000);
        // Once a second while a game runs: processor time per thread, and memory. Every 5 s: the
        // game thread's profile.
        {
            static uint64_t last_report, last_frame_seen;
            uint64_t t = now_ns();
            if (t - last_report >= 1000000000ull) {
                last_report = t;
                if (g_frame_number != last_frame_seen) {
                    last_frame_seen = g_frame_number;
                    threads_report();
                    if (g_prof_since == 0) g_prof_since = t;
                    if (t - g_prof_since >= 5000000000ull) {
                        prof_report();
                        g_prof_since = t;
                    }
                } else {
                    g_threads_at = 0;
                    g_prof_n = 0;
                    g_prof_since = 0;
                }
            }
        }
        uint64_t start = __atomic_load_n(&g_frame_start, __ATOMIC_ACQUIRE);
        if (start == 0) continue;
        prof_sample();
        uint64_t frame = g_frame_number;
        uint64_t now = now_ns();
        uint64_t elapsed = now > start ? now - start : 0;
        if (now / 1000000000ull != second) {
            second = now / 1000000000ull;
            this_second = 0;
        }
        if (elapsed >= STUCK_NS) {
            if (now - last_stuck_sample >= 2000ull * 1000000ull) {
                last_stuck_sample = now;
                sample_game_thread("STUCK", frame, elapsed);
            }
        } else if (elapsed >= SLOW_NS && frame != sampled_frame && g_slow_samples < MAX_SLOW_SAMPLES &&
                   this_second < 4) {
            sampled_frame = frame;
            g_slow_samples++;
            this_second++;
            sample_game_thread("slow", frame, elapsed);
        }
    }
    return NULL;
}

// --------------------------------------------------------------------------- the crash catcher

static const int g_signals[] = {SIGSEGV, SIGBUS, SIGILL, SIGTRAP, SIGABRT, SIGFPE};
#define NSIG_CAUGHT (sizeof g_signals / sizeof g_signals[0])
static struct sigaction g_previous[NSIG_CAUGHT];
static volatile sig_atomic_t g_in_crash;

static const char *signal_name(int sig) {
    switch (sig) {
    case SIGSEGV: return "SIGSEGV (bad memory access)";
    case SIGBUS: return "SIGBUS (bad memory access)";
    case SIGILL: return "SIGILL (illegal instruction)";
    case SIGTRAP: return "SIGTRAP (breakpoint or trap)";
    case SIGABRT: return "SIGABRT (abort)";
    case SIGFPE: return "SIGFPE (arithmetic)";
    default: return "signal";
    }
}

static void crash_report(int fd, int sig, siginfo_t *info, ucontext_t *uc) {
    put(fd, "\n==== CRASH ");
    put_time(fd);
    put(fd, signal_name(sig));
    put(fd, " code ");
    put_dec(fd, (uint64_t)(info ? info->si_code : 0));
    put(fd, " address ");
    put_hex(fd, info ? (uint64_t)info->si_addr : 0);
    put(fd, "\nthread: ");
    put(fd, pthread_equal(pthread_self(), g_game_pthread) ? "the game thread" : "another thread");
    char name[64] = {0};
    pthread_getname_np(pthread_self(), name, sizeof name);
    if (name[0]) {
        put(fd, " (");
        put(fd, name);
        put(fd, ")");
    }
    put(fd, ", game frame ");
    put_dec(fd, g_frame_number);
    put(fd, g_frame_start ? " (inside the frame)\n" : " (between frames)\n");
    if (uc == NULL || uc->uc_mcontext == NULL) return;
    arm_thread_state64_t *ss = &uc->uc_mcontext->__ss;
    uint64_t pc = (uint64_t)arm_thread_state64_get_pc(*ss) & 0x0000FFFFFFFFFFFFull;
    uint64_t lr = (uint64_t)arm_thread_state64_get_lr(*ss) & 0x0000FFFFFFFFFFFFull;
    uint64_t fp = (uint64_t)arm_thread_state64_get_fp(*ss);
    uint64_t sp = (uint64_t)arm_thread_state64_get_sp(*ss);
    put(fd, "pc ");
    put_hex(fd, pc);
    put(fd, " lr ");
    put_hex(fd, lr);
    put(fd, " sp ");
    put_hex(fd, sp);
    put(fd, " fp ");
    put_hex(fd, fp);
    put(fd, "\nregisters:");
    for (int i = 0; i < 29; i++) {
        put(fd, i % 4 == 0 ? "\n  " : "  ");
        put(fd, "x");
        put_dec(fd, (uint64_t)i);
        put(fd, "=");
        put_hex(fd, ss->__x[i]);
    }
    put(fd, "\nwhere:\n    pc ");
    put_where(fd, pc);
    put(fd, "\n    lr ");
    put_where(fd, lr);
    put(fd, "\n");
    // Callers, from the frame pointers, kept inside this thread's stack.
    uint64_t hi = (uint64_t)pthread_get_stackaddr_np(pthread_self());
    uint64_t lo = hi - (uint64_t)pthread_get_stacksize_np(pthread_self());
    for (int i = 0; i < 40 && fp >= lo && fp + 16 <= hi && (fp & 7) == 0; i++) {
        uint64_t next = ((uint64_t *)fp)[0];
        uint64_t ret = ((uint64_t *)fp)[1] & 0x0000FFFFFFFFFFFFull;
        if (ret == 0) break;
        put(fd, "    <- ");
        put_where(fd, ret);
        put(fd, "\n");
        if (next <= fp) break;
        fp = next;
    }
}

static void on_crash(int sig, siginfo_t *info, void *raw) {
    if (!g_in_crash) {
        g_in_crash = 1;
        crash_report(g_crash_fd, sig, info, (ucontext_t *)raw);
        put(g_log_fd, "==== CRASH (the details are in crash.txt)\n");
        crash_report(g_log_fd, sig, info, (ucontext_t *)raw);
    }
    // Hand it on: the handler that was there before, else the default, which ends the app the way
    // it always did (and the next start offers the crash report).
    for (size_t i = 0; i < NSIG_CAUGHT; i++) {
        if (g_signals[i] != sig) continue;
        struct sigaction *prev = &g_previous[i];
        if ((prev->sa_flags & SA_SIGINFO) && prev->sa_sigaction != NULL) {
            sigaction(sig, prev, NULL);
            prev->sa_sigaction(sig, info, raw);
            return;
        }
        if (prev->sa_handler != SIG_DFL && prev->sa_handler != SIG_IGN && prev->sa_handler != NULL) {
            sigaction(sig, prev, NULL);
            prev->sa_handler(sig);
            return;
        }
    }
    signal(sig, SIG_DFL);
    if (sig == SIGABRT) raise(sig);
    // Returning re-runs the faulting instruction with the default action in place.
}

// ---------------------------------------------------------------------------------- start

void continuum_tel_open(const char *dir) {
    if (g_log_fd >= 0 || dir == NULL) return;
    mach_timebase_info(&g_tb);
    g_t0 = now_ns();
    strncpy(g_dir, dir, sizeof g_dir - 1);
    mkdir(g_dir, 0755);
    rotate("core-log");
    rotate("crash");
    char path[1200];
    snprintf(path, sizeof path, "%s/core-log.txt", g_dir);
    g_log_fd = open(path, O_WRONLY | O_CREAT | O_TRUNC | O_APPEND | O_CLOEXEC, 0644);
    snprintf(path, sizeof path, "%s/crash.txt", g_dir);
    g_crash_fd = open(path, O_WRONLY | O_CREAT | O_TRUNC | O_APPEND | O_CLOEXEC, 0644);

    // An alternate stack for the main thread, so a stack overflow there can still be written down.
    static char alt[64 * 1024];
    stack_t ss = {.ss_sp = alt, .ss_size = sizeof alt, .ss_flags = 0};
    sigaltstack(&ss, NULL);
    for (size_t i = 0; i < NSIG_CAUGHT; i++) {
        struct sigaction sa;
        memset(&sa, 0, sizeof sa);
        sa.sa_sigaction = on_crash;
        sa.sa_flags = SA_SIGINFO | SA_ONSTACK;
        sigemptyset(&sa.sa_mask);
        sigaction(g_signals[i], &sa, &g_previous[i]);
    }
    pthread_t t;
    pthread_attr_t attr;
    pthread_attr_init(&attr);
    pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED);
    pthread_create(&t, &attr, watchdog, NULL);
    pthread_attr_destroy(&attr);
    log_line("app: ", "telemetry on (core log, crash catcher, frame watchdog)");
}

#else // not Apple: the host test machine. Lines go to stderr, nothing else runs.

#include <stdio.h>

void continuum_tel_open(const char *dir) { (void)dir; }
void continuum_tel_core_log(int level, const char *text) { (void)level; (void)text; }
void continuum_tel_note(const char *text) { (void)text; }
void continuum_tel_frame_begin(void) {}
void continuum_tel_frame_end(void) {}

#endif
