// A pretend Objective-C runtime for testing AppleHudOff.lua on Linux x86_64.
// One objc_msgSend handles every call shape the module uses (on x86_64 the
// pointer/bool/integer arguments land in the same registers either way).
// Object tree: app -> windows [win] / connectedScenes {scene -> windows [win]}
//              win.layer = root CALayer -> sublayers [metal CAMetalLayer, plain CALayer]
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct Obj {
    const char *cls;
    struct Obj *items[8];
    int nitems;
    int is_array;
    struct Obj *layer, *sublayers, *props, *windows;
    const char *str;
    int count;  // dictionaries
} Obj;

static Obj CLS_NSString = {"Class:NSString"}, CLS_UIApplication = {"Class:UIApplication"},
           CLS_NSDictionary = {"Class:NSDictionary"}, CLS_CAMetalLayer = {"Class:CAMetalLayer"},
           CLS_NSUserDefaults = {"Class:NSUserDefaults"};
static Obj app = {"UIApplication"}, win = {"UIWindow"}, scene = {"UIWindowScene"},
           scenes = {"NSSet"}, scenes_list = {"NSArray", .is_array = 1},
           wins = {"NSArray", .is_array = 1}, root = {"CALayer"}, root_subs = {"NSArray", .is_array = 1},
           metal = {"CAMetalLayer"}, plain = {"CALayer"}, ud = {"NSUserDefaults"},
           system_props = {"NSDictionary", .count = 1};

static char *sels[64];
static int nsels;
int pool_depth, pool_max, set_calls, set_with_empty, synchronize_calls;
int force_hud_value = -1, notice_flag = 0;

void *sel_registerName(const char *name) {
    for (int i = 0; i < nsels; i++) if (!strcmp(sels[i], name)) return sels[i];
    sels[nsels] = strdup(name);
    return sels[nsels++];
}

void *objc_getClass(const char *name) {
    if (!strcmp(name, "NSString")) return &CLS_NSString;
    if (!strcmp(name, "UIApplication")) return &CLS_UIApplication;
    if (!strcmp(name, "NSDictionary")) return &CLS_NSDictionary;
    if (!strcmp(name, "CAMetalLayer")) return &CLS_CAMetalLayer;
    if (!strcmp(name, "NSUserDefaults")) return &CLS_NSUserDefaults;
    return NULL;
}

void *objc_autoreleasePoolPush(void) { pool_depth++; if (pool_depth > pool_max) pool_max = pool_depth; return (void *)(uintptr_t)pool_depth; }
void objc_autoreleasePoolPop(void *p) { (void)p; pool_depth--; }

static void setup(void) {
    static int done;
    if (done) return;
    done = 1;
    wins.items[0] = &win; wins.nitems = 1;
    scenes_list.items[0] = &scene; scenes_list.nitems = 1;
    scene.windows = &wins;
    win.layer = &root;
    root_subs.items[0] = &metal; root_subs.items[1] = &plain; root_subs.nitems = 2;
    root.sublayers = &root_subs;
    metal.props = &system_props;  // pretend the system asked for the HUD
}

uintptr_t objc_msgSend(void *self_, void *sel_, uintptr_t a, uintptr_t b) {
    setup();
    Obj *self = self_;
    const char *s = sel_;
    if (!self) return 0;
    if (!strcmp(s, "stringWithUTF8String:")) { Obj *o = calloc(1, sizeof(Obj)); o->cls = "NSString"; o->str = strdup((const char *)a); return (uintptr_t)o; }
    if (!strcmp(s, "sharedApplication")) return (uintptr_t)&app;
    if (!strcmp(s, "windows")) return (uintptr_t)(self == &app ? &wins : self->windows);
    if (!strcmp(s, "connectedScenes")) return (uintptr_t)&scenes;
    if (!strcmp(s, "allObjects")) return (uintptr_t)&scenes_list;
    if (!strcmp(s, "count")) return self->is_array ? (uintptr_t)self->nitems : (uintptr_t)self->count;
    if (!strcmp(s, "objectAtIndex:")) return a < (uintptr_t)self->nitems ? (uintptr_t)self->items[a] : 0;
    if (!strcmp(s, "layer")) return (uintptr_t)self->layer;
    if (!strcmp(s, "sublayers")) return (uintptr_t)self->sublayers;
    if (!strcmp(s, "isKindOfClass:")) return a == (uintptr_t)&CLS_CAMetalLayer && !strcmp(self->cls, "CAMetalLayer");
    if (!strcmp(s, "respondsToSelector:")) {
        const char *q = (const char *)a;
        if (!strcmp(q, "setDeveloperHUDProperties:")) return !strcmp(self->cls, "CAMetalLayer");
        if (!strcmp(q, "windows")) return self == &scene;
        return 0;
    }
    if (!strcmp(s, "developerHUDProperties")) return (uintptr_t)self->props;
    if (!strcmp(s, "setDeveloperHUDProperties:")) {
        set_calls++;
        self->props = (Obj *)a;
        if (self->props && !strcmp(self->props->cls, "NSDictionary") && self->props->count == 0) set_with_empty++;
        return 0;
    }
    if (!strcmp(s, "alloc") && self == &CLS_NSDictionary) { Obj *o = calloc(1, sizeof(Obj)); o->cls = "NSDictionary"; return (uintptr_t)o; }
    if (!strcmp(s, "init")) return (uintptr_t)self;
    if (!strcmp(s, "standardUserDefaults")) return (uintptr_t)&ud;
    if (!strcmp(s, "setBool:forKey:")) {
        Obj *k = (Obj *)b;
        if (!strcmp(k->str, "MetalForceHudEnabled")) force_hud_value = (int)(a & 0xff);
        else notice_flag = (int)(a & 0xff);
        return 0;
    }
    if (!strcmp(s, "boolForKey:")) return (uintptr_t)notice_flag;
    if (!strcmp(s, "synchronize")) { synchronize_calls++; return 1; }
    fprintf(stderr, "UNHANDLED %s on %s\n", s, self->cls);
    exit(3);
}

void put_back(void) { metal.props = &system_props; }

int report(char *buf, int n) {
    return snprintf(buf, n, "pool_depth=%d pool_max=%d set_calls=%d set_with_empty=%d force_hud=%d notice_flag=%d sync=%d",
                    pool_depth, pool_max, set_calls, set_with_empty, force_hud_value, notice_flag, synchronize_calls);
}
