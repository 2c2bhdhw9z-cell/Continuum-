// Stand-ins for PokeMini's screen scaler, which the headless harness doesn't use.
int PokeMini_VideoDepth = 16;
void *PokeMini_VideoCurrent = 0;
int VidEnableHighcolor = 0;
int PokeMini_SetVideo(void *spec, int bpp, int filter, int lcdmode) { (void) spec; (void) bpp; (void) filter; (void) lcdmode; return 1; }
