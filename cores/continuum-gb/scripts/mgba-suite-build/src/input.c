// Keypad polling, libgba semantics: REG_KEYINPUT is active low, so a pressed key
// reads as a 0 bit. keysDown is the newly-pressed set, keysDownRepeat repeats a held
// key after `delay` scans and then every `repeat` scans (setRepeat configures both).
#include <gba_input.h>

static u16 keysCur;
static u16 keysPrev;
static int repeatDelay = 20;
static int repeatRate = 6;
static int repeatCounter;

void scanKeys(void) {
    keysPrev = keysCur;
    keysCur = (u16)(~REG_KEYINPUT & KEY_ANY);
}

u16 keysDown(void) {
    return (u16)(keysCur & ~keysPrev);
}

u16 keysUp(void) {
    return (u16)(~keysCur & keysPrev);
}

u16 keysHeld(void) {
    return keysCur;
}

u16 keysDownRepeat(void) {
    u16 down = keysDown();
    if (down) {
        repeatCounter = 0;
        return down;
    }
    if (!keysCur) {
        repeatCounter = 0;
        return 0;
    }
    if (++repeatCounter >= repeatDelay) {
        repeatCounter = repeatDelay - repeatRate;
        return keysCur;
    }
    return 0;
}

void setRepeat(int delay, int repeat) {
    repeatDelay = delay;
    repeatRate = repeat;
}
