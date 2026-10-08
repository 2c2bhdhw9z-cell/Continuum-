// iOS 26 JIT on a phone with TXM: ask the debugger to bless one big region, once.
//
// WHY THIS FILE EXISTS
//
// On an iPhone up to iOS 18 (and on older phones on iOS 26), a JIT enabler attaching a debugger
// is the whole story: after that the app may make memory executable and every recompiler works.
// On iOS 26 with TXM (Trusted Execution Monitor: iPhone 13 and newer), that is no longer enough.
// Each region of executable memory must be PREPARED through the debug connection before the app
// runs code from it, and it stays read-and-execute afterwards: writes have to go through a second
// address that points at the same memory.
//
// That protocol is StikJIT's "universal" script (its INTEGRATION.md, Part 1). The app calls two
// functions that are nothing but a breakpoint; the script attached on the other side sees it and
// does the work:
//
//     JIT26Detach()                      x16 = 0
//     JIT26PrepareRegion(address, size)   x16 = 1, returns the prepared address in x0
//
// THE BREAKPOINT IS ONLY EVER EXECUTED WITH THAT SCRIPT ATTACHED. A `brk` with nothing listening
// terminates the process. `continuum_jit26_prepare` is therefore called from exactly one place in
// Rust (`crate::jit::prepare_for_txm`), which checks CS_DEBUGGED first and only runs at all when
// the app itself asked StikDebug for the universal script, or the user pressed a button that says
// what it needs. See the guard comment on that function.
//
// HOW THE MEMORY IS SHAPED
//
// One region is reserved and prepared up front, before detaching, because a region introduced
// after the script has let go cannot be prepared any more. Cores then take slices of it through
// `continuum_jit_region`, which hands back two addresses for the same memory: `rx` to run code
// from and `rw` to write it through. Every core this project builds with a recompiler already
// understands that shape, because it is how they run on the Nintendo Switch.
//
// Same approach as DolphiniOS (MemoryUtil_iOS_LuckTXM.cpp): mmap read+execute, prepare it, then
// `vm_remap` a second writable view of the same pages.

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#if defined(__APPLE__) && defined(__aarch64__)
#include <TargetConditionals.h>
#endif

// Everything below is iPhone-only. A host build (the Linux test machine) compiles the stubs at the
// bottom instead, so this file is harmless there rather than conditionally absent from the build.
#if defined(__APPLE__) && defined(__aarch64__) && TARGET_OS_IPHONE

#include <mach/mach.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

// The two breakpoint calls, exactly as StikJIT's guide defines them. `naked` means the compiler
// writes no prologue: the arguments stay in x0 and x1 where the script expects them, and x0 is
// still the return value. Do not add code to these.
__attribute__((noinline, optnone, naked)) static void continuum_jit26_detach_call(void) {
    __asm__ volatile("mov x16, #0\n"
                     "brk #0xf00d\n"
                     "ret\n");
}

__attribute__((noinline, optnone, naked)) static void *
continuum_jit26_prepare_call(void *address, size_t length) {
    __asm__ volatile("mov x16, #1\n"
                     "brk #0xf00d\n"
                     "ret\n");
}

// 0 not attempted, 1 ready, 2 failed. Read through `continuum_jit26_state`.
static int g_state = 0;
static const char *g_error = "";

static uint8_t *g_rx = NULL;
static uint8_t *g_rw = NULL;
static size_t g_size = 0;
static size_t g_used = 0;

// Whether JIT is actually on right now, set by the engine before any core loads. Nothing is
// handed out when it is false: a core that asked anyway would be given memory it is not allowed
// to run code from, and the failure would land inside the core rather than here.
static bool g_live = false;

// One slice per asking core, remembered by name.
//
// A core is unloaded and loaded again every time the user leaves a game and opens another, and it
// asks for its code memory each time. Without this the arena would be eaten a slice at a time
// until it ran out mid-session; with it, the same core always gets the same slice back. The names
// are short fixed strings from the patches, so there is no allocation here.
#define CONTINUUM_JIT_SLOTS 32
static struct {
    char owner[32];
    uint8_t *rx;
    uint8_t *rw;
    size_t size;
    bool in_use;
} g_slots[CONTINUUM_JIT_SLOTS];
static int g_slot_count = 0;

static size_t continuum_jit_page_round(size_t value) {
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    if (page == 0) {
        page = 16384;
    }
    return (value + page - 1) & ~(page - 1);
}

/// A writable second address for pages that are already read-and-execute. The whole trick, used
/// both for the blessed region and for a plain mapping on a phone that needs no blessing.
static uint8_t *continuum_jit_writable_view(uint8_t *rx, size_t size) {
    vm_address_t writable = 0;
    vm_prot_t current_protection = 0;
    vm_prot_t max_protection = 0;
    kern_return_t result =
        vm_remap(mach_task_self(), &writable, size, 0, VM_FLAGS_ANYWHERE, mach_task_self(),
                 (vm_address_t)rx, false, &current_protection, &max_protection, VM_INHERIT_DEFAULT);
    if (result != KERN_SUCCESS) {
        return NULL;
    }
    if (mprotect((void *)writable, size, PROT_READ | PROT_WRITE) != 0) {
        return NULL;
    }
    return (uint8_t *)writable;
}

/// Reserves `size` bytes, has the debugger prepare them, and maps a writable view of the same
/// pages. Returns true when both addresses are usable.
///
/// SAFE TO CALL ONLY WITH THE UNIVERSAL SCRIPT ATTACHED. See the file header.
bool continuum_jit26_prepare(size_t size) {
    if (g_state == 1) {
        return true;
    }
    if (g_state == 2) {
        return false;
    }

    size = continuum_jit_page_round(size);

    // Read and execute, never written through this address. Not MAP_JIT: that flag is macOS's
    // answer to the same problem and is not what the iOS 26 protocol uses.
    uint8_t *rx = (uint8_t *)mmap(NULL, size, PROT_READ | PROT_EXEC, MAP_ANON | MAP_PRIVATE, -1, 0);
    if (rx == MAP_FAILED || rx == NULL) {
        g_state = 2;
        g_error = "the code region could not be reserved";
        // Let the debugger go here too, as every other failure does: the script is attached and
        // waiting, and nothing is gained by leaving the phone attached to it.
        continuum_jit26_detach_call();
        return false;
    }

    // The debugger blesses the region. Passing our own address rather than NULL, so the region
    // stays where it was reserved and the writable view below is a remap of known pages.
    void *prepared = continuum_jit26_prepare_call(rx, size);
    if (prepared == NULL) {
        // NULL is the script saying no, not "keep your own address": nothing was blessed, so
        // nothing here may ever run code. Give the reservation back and let the debugger go.
        munmap(rx, size);
        g_state = 2;
        g_error = "the debugger did not prepare the code region";
        continuum_jit26_detach_call();
        return false;
    }
    if (prepared != rx) {
        // The script chose a different address; believe it and release ours.
        munmap(rx, size);
        rx = (uint8_t *)prepared;
    }

    // A second address for the same physical memory, writable. This is what makes the region
    // usable at all: the prepared mapping itself must stay read-and-execute.
    uint8_t *writable = continuum_jit_writable_view(rx, size);
    if (writable == NULL) {
        g_state = 2;
        g_error = "a writable view of the code region was refused";
        continuum_jit26_detach_call();
        return false;
    }

    g_rx = rx;
    g_rw = writable;
    g_size = size;
    g_used = 0;
    g_slot_count = 0;
    memset(g_slots, 0, sizeof(g_slots));

    // Let the debugger go. Nothing may be prepared after this, which is why the whole region is
    // taken in one piece above.
    continuum_jit26_detach_call();

    g_state = 1;
    g_error = "";
    return true;
}

int continuum_jit26_state(void) { return g_state; }

/// The engine says whether JIT is on, before a core is loaded.
void continuum_jit26_set_live(bool live) { g_live = live; }

const char *continuum_jit26_error(void) { return g_error; }

size_t continuum_jit26_region_size(void) { return g_size; }

size_t continuum_jit26_used(void) { return g_used; }

/// THE SYMBOL THE PATCHED CORES LOOK UP. Hands out one slice of the prepared region: `rx` to run
/// code from, `rw` to write it through, the same memory at two addresses.
///
/// Returns false when there is no prepared region (every phone that does not need this protocol,
/// which is most of them) or the arena is full. Every caller treats false as "carry on the way you
/// always did", so a core is never worse off for asking.
///
/// `used` and default visibility keep it in the app's symbol table: the engine is linked into the
/// executable as a static library, and the cores are separate dynamic libraries that find this
/// through `dlsym(RTLD_DEFAULT, "continuum_jit_region")`.
__attribute__((used, visibility("default"))) bool
continuum_jit_region(const char *owner, size_t size, void **out_rx, void **out_rw) {
    if (!g_live || out_rx == NULL || out_rw == NULL || size == 0) {
        return false;
    }
    const char *name = (owner != NULL) ? owner : "unnamed";
    size = continuum_jit_page_round(size);

    // A slice this core already holds, by name. A core asks again every time a game is opened, so
    // handing the same memory back is what stops the region draining one game at a time.
    for (int i = 0; i < g_slot_count; i++) {
        if (g_slots[i].in_use && size <= g_slots[i].size &&
            strncmp(g_slots[i].owner, name, sizeof(g_slots[i].owner) - 1) == 0) {
            *out_rx = g_slots[i].rx;
            *out_rw = g_slots[i].rw;
            return true;
        }
    }

    // A slice someone released on shutdown, big enough to be used again: the SMALLEST that fits.
    // The whole slice is handed over, so first-fit let a tiny request take a 32 MB slice and left
    // the next big core nothing to reuse.
    int best = -1;
    for (int i = 0; i < g_slot_count; i++) {
        if (!g_slots[i].in_use && size <= g_slots[i].size &&
            (best < 0 || g_slots[i].size < g_slots[best].size)) {
            best = i;
        }
    }
    if (best >= 0) {
        strncpy(g_slots[best].owner, name, sizeof(g_slots[best].owner) - 1);
        g_slots[best].owner[sizeof(g_slots[best].owner) - 1] = '\0';
        g_slots[best].in_use = true;
        *out_rx = g_slots[best].rx;
        *out_rw = g_slots[best].rw;
        return true;
    }

    if (g_slot_count >= CONTINUUM_JIT_SLOTS) {
        return false;
    }

    uint8_t *rx = NULL;
    uint8_t *rw = NULL;
    if (g_state == 1) {
        // Carve the next slice off the blessed region. Nothing outside it can run code on this
        // phone, so there is no fallback here.
        if (size > g_size - g_used) {
            return false;
        }
        rx = g_rx + g_used;
        rw = g_rw + g_used;
        g_used += size;
    } else {
        // A phone that needs no blessing: a mapping of its own, in the same two-address shape, so
        // that every core takes one code path on every phone.
        rx = (uint8_t *)mmap(NULL, size, PROT_READ | PROT_EXEC, MAP_ANON | MAP_PRIVATE, -1, 0);
        if (rx == MAP_FAILED || rx == NULL) {
            return false;
        }
        rw = continuum_jit_writable_view(rx, size);
        if (rw == NULL) {
            munmap(rx, size);
            return false;
        }
    }

    int slot = g_slot_count++;
    strncpy(g_slots[slot].owner, name, sizeof(g_slots[slot].owner) - 1);
    g_slots[slot].owner[sizeof(g_slots[slot].owner) - 1] = '\0';
    g_slots[slot].rx = rx;
    g_slots[slot].rw = rw;
    g_slots[slot].size = size;
    g_slots[slot].in_use = true;

    *out_rx = rx;
    *out_rw = rw;
    return true;
}

/// Says a slice is no longer needed, so the next core to ask can have it. The patched cores call
/// this when they shut their recompiler down. Safe with any pointer, including one that never
/// came from here.
///
/// Either address of a slice is accepted, the one it runs from or the one it is written through,
/// because some cores only keep the writing one (flycast's recompilers do). For a slice of the
/// blessed region the writing address is the writable view's base plus the slice's offset into the
/// region, which is exactly what `rw` was set to when the slice was carved.
__attribute__((used, visibility("default"))) void continuum_jit_release(void *address) {
    if (address == NULL) {
        return;
    }
    for (int i = 0; i < g_slot_count; i++) {
        if (g_slots[i].rx == (uint8_t *)address || g_slots[i].rw == (uint8_t *)address) {
            g_slots[i].in_use = false;
            return;
        }
    }
}

#else // not an iPhone build

bool continuum_jit26_prepare(size_t size) {
    (void)size;
    return false;
}

int continuum_jit26_state(void) { return 0; }

void continuum_jit26_set_live(bool live) { (void)live; }

const char *continuum_jit26_error(void) { return "not an iPhone build"; }

size_t continuum_jit26_region_size(void) { return 0; }

size_t continuum_jit26_used(void) { return 0; }

__attribute__((used, visibility("default"))) bool
continuum_jit_region(const char *owner, size_t size, void **out_rx, void **out_rw) {
    (void)owner;
    (void)size;
    (void)out_rx;
    (void)out_rw;
    return false;
}

__attribute__((used, visibility("default"))) void continuum_jit_release(void *rx) { (void)rx; }

#endif
