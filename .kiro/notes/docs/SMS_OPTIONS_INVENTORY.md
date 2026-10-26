# Sega 8-bit core (S1, working name continuum-sms): settings inventory

Every setting the emulators it must match offer, and what the new core does with each. Written before any
code, as cBoy's and the GameCube's were. "planned": the new core will have it; "app": the Continuum app
already does it for every core; "n/a": not about these consoles. Sources, at the commits read
(25 October 2026):

- Gearsystem (libretro; the app's current core, GPL v3): 28 settings, 27 planned, 1 app, 0 n/a. [215c22f](https://github.com/drhelius/Gearsystem/blob/215c22f73d06871e12291025105c98470509468d/platforms/libretro/libretro_core_options.h)
- Genesis Plus GX (libretro): 62 settings, 36 planned, 3 app, 23 n/a. [60bee4c](https://github.com/libretro/Genesis-Plus-GX/blob/60bee4c724859710ce05672239e5b601ad01382e/libretro/libretro_core_options.h)
- PicoDrive (libretro): 21 settings, 12 planned, 3 app, 6 n/a. [1890c29](https://github.com/libretro/picodrive/blob/1890c2932234c9d30f4cd3851d02228baae8f09e/platform/libretro/libretro_core_options.h)
- SMS Plus GX (libretro): 6 settings, 6 planned, 0 app, 0 n/a. [3844b46](https://github.com/libretro/smsplus-gx/blob/3844b46caa926b6494987b97da63092818c4ddef/source/ports/libretro/libretro_core_options.h)
- Gearsystem standalone (desktop) adds, outside its libretro list: run-ahead, rewind (seconds, frames per
  snapshot, speed), fast-forward speed, soft patching (IPS/BPS), master volume, sound buffer, shader presets,
  window and debugger settings. Run-ahead, rewind, fast forward, shaders and patches are the app's; the
  debugger and window ones are n/a. [config_definitions.inc.h](https://github.com/drhelius/Gearsystem/blob/215c22f73d06871e12291025105c98470509468d/platforms/shared/desktop/config_definitions.inc.h)
- Emulicious publishes no settings list, only features (adjustable turbo and speed, IPS patching, save
  states) and debugging tools ([emulicious.net](https://emulicious.net/)): turbo is planned, the rest is the app's or n/a.

## In cGear so far (25 October)

14 settings (`cores/continuum-sms/src/options.rs`): console (Auto, Master System, Game Gear, SG-1000),
region (export or Japan, which games read through port $3F), refresh rate (60 or 50 Hz), cartridge
wiring (Auto, Sega, Codemasters, Korean), eight sprites a line, hide the left column, Game Gear screen
ghosting, volume, each of the four sound channels' volume, mono, and opposite directions. Covers these
rows below: gearsystem_system, _region, _timing, _mapper, _hide_left_bar, _no_sprite_limit,
_psg_volume, _up_down_allowed; genesis_plus_gx_system_hw, _region_detect, _vdp_mode, _left_border,
_lcd_filter, _no_sprite_limit, _psg_preamp, _psg_channel_0-3_volume, _sound_output; picodrive_region,
_smstype, _smsmapper, _ggghost, _sprlim; smsplus_hardware, _region, _hide_left_border.

## Our own, on top (the cBoy way)

- Every console and revision as its own choice: SG-1000, SG-1000 II, SC-3000, Mark III, Master System
  (Japanese, export, PAL), Master System II, Game Gear (Japanese, export), Genesis/Mega Drive in its
  Master System mode (its VDP draws a few modes differently), and the Master System's start-up file (BIOS)
  when the player has one, with the built-in start when not.
- Channel-by-channel volume for the SN76489 and each YM2413 channel (Genesis Plus GX has it), the FM chip
  on or off per game, stereo for the Game Gear.
- Game Gear LCD ghosting and colours as the screen showed them, Master System colours as a TV showed them.
- Light phaser and paddle on the touch screen, the Game Gear Start button on the pad, rapid fire.
- Two consoles on one phone and the Game Gear cable to a second phone, the way cBoy does both.

## Key by key

| Emulator | Key | Label | New core | Note | Values |
|---|---|---|---|---|---|
| gearsystem | `gearsystem_system` | System (restart) | planned |  | Auto,Master System / Mark III,Game Gear (2 ASIC),Game Gear (2 ASIC) SMS Mode,Game Gear (1 ASIC),Game Gear (1 ASIC) SMS Mode,SG-1000 / Multivision,SG-1000 II |
| gearsystem | `gearsystem_region` | Region (restart) | done | `cgear_region` Auto (header region code, file name), Export, Japan | Auto,Master System Japan,Master System Export,Game Gear Japan,Game Gear Export,Game Gear International |
| gearsystem | `gearsystem_mapper` | Mapper (restart) | done | `cgear_mapper` + Korean MSX, Nemesis, Janggun (no Korean 2000 XOR / MSX 32KB variants yet) | Auto,ROM,SEGA,Codemasters,Korean,SG-1000,MSX,Janggun,Korean 2000 XOR 1F,Korean MSX 32KB 2000,... |
| gearsystem | `gearsystem_timing` | Refresh Rate (restart) | done | `cgear_refresh` 60, 50, Auto (file name) | Auto,NTSC (60 Hz),PAL (50 Hz) |
| gearsystem | `gearsystem_bios_sms` | Master System BIOS (restart) | planned |  | Disabled,Enabled |
| gearsystem | `gearsystem_bios_gg` | Game Gear BIOS (restart) | planned |  | Disabled,Enabled |
| gearsystem | `gearsystem_aspect_ratio` | Aspect Ratio | app | the app has its own picture shape setting | 1:1 PAR,4:3 DAR,16:9 DAR,16:10 DAR |
| gearsystem | `gearsystem_overscan` | Overscan | done | `cgear_borders` (Off, Above and below, All round = 284 wide) | Disabled,Top+Bottom,Full (284 width),Full (320 width) |
| gearsystem | `gearsystem_hide_left_bar` | Hide Left Bar (SMS only) | planned |  | No,Auto,Always |
| gearsystem | `gearsystem_no_sprite_limit` | No Sprite Limit | done | `cgear_sprite_limit` | Disabled,Enabled |
| gearsystem | `gearsystem_glasses` | 3D Glasses | done | `cgear_3d` (Both eyes, Left eye, Right eye) | Both Eyes / OFF,Left Eye,Right Eye |
| gearsystem | `gearsystem_ym2413` | YM2413 (restart) | done | `cgear_fm` (Auto/Off/On) | Auto,Disabled |
| gearsystem | `gearsystem_psg_volume` | PSG Volume | done | `cgear_volume` | 0,10,20,30,40,50,60,70,80,90,... |
| gearsystem | `gearsystem_fm_volume` | FM Volume | done | `cgear_fm_volume` | 0,10,20,30,40,50,60,70,80,90,... |
| gearsystem | `gearsystem_up_down_allowed` | Allow Up+Down / Left+Right | done | `cgear_opposing` | Disabled,Enabled |
| gearsystem | `gearsystem_lightgun_input` | Light Gun Input | done | `cgear_port1`/`cgear_port2` = Light Phaser; stick, D-pad, or a touch once the app sends one | Light Gun,Touchscreen |
| gearsystem | `gearsystem_lightgun_crosshair` | Light Gun Crosshair | done | `cgear_crosshair` | Disabled,Enabled |
| gearsystem | `gearsystem_lightgun_shape` | Light Gun Crosshair Shape | done | `cgear_crosshair_shape` | Cross,Square |
| gearsystem | `gearsystem_lightgun_color` | Light Gun Crosshair Color | done | `cgear_crosshair_colour` | White,Black,Red,Green,Blue,Yellow,Magenta,Cyan |
| gearsystem | `gearsystem_lightgun_crosshair_offset_x` | Light Gun Crosshair Offset X | done | `cgear_phaser_x` | -10,-9,-8,-7,-6,-5,-4,-3,-2,-1,... |
| gearsystem | `gearsystem_lightgun_crosshair_offset_y` | Light Gun Crosshair Offset Y | done | `cgear_phaser_y` | -10,-9,-8,-7,-6,-5,-4,-3,-2,-1,... |
| gearsystem | `gearsystem_paddle_sensitivity` | Paddle Sensitivity | done | `cgear_device_speed` | 1,2,3,4,5,6,7,8,9,10,... |
| gearsystem | `gearsystem_sports_pad_sensitivity` | Sports Pad Sensitivity | done | `cgear_device_speed` | 1,2,3,4,5,6,7,8,9,10,... |
| gearsystem | `gearsystem_link_enable` | Game Link Cable Enable (restart) | done | `cgear_gg_link` (two Game Gears on one phone; not yet two phones) | Enabled,Disabled |
| gearsystem | `gearsystem_link_placement` | Dual Screen Placement | done | `cgear_gg_link_layout` | Horizontal,Vertical |
| gearsystem | `gearsystem_link_switch` | Dual Screen Switch | done | `cgear_gg_link_swap` | Enabled,Disabled |
| gearsystem | `gearsystem_link_screen` | Dual Screen Selection | done | `cgear_gg_link_show` | Both Screens,Screen 1,Screen 2 |
| gearsystem | `gearsystem_link_audio` | Dual Screen Audio | done | `cgear_gg_link_audio` | Screen 1,Screen 2,Mix |
| gpgx | `genesis_plus_gx_system_hw` | System Hardware | planned |  | auto,sg-1000,sg-1000 II,sg-1000 II + ram ext.,mark-III,master system,master system II,game gear,mega drive / genesis |
| gpgx | `genesis_plus_gx_region_detect` | System Region | done | `cgear_region` + `cgear_refresh` | auto,ntsc-u,pal,ntsc-j |
| gpgx | `genesis_plus_gx_vdp_mode` | Force VDP Mode | done | `cgear_refresh` | auto,60hz,50hz |
| gpgx | `genesis_plus_gx_bios` | System Boot ROM | planned |  | disabled,enabled |
| gpgx | `genesis_plus_gx_system_bram` | CD System BRAM (Requires Restart) | n/a | Mega Drive or Mega-CD only | per bios,per game |
| gpgx | `genesis_plus_gx_cart_bram` | CD Backup Cart BRAM (Requires Restart) | n/a | Mega Drive or Mega-CD only | per cart,per game |
| gpgx | `genesis_plus_gx_cart_size` | CD Backup Cart BRAM Size (Requires Restart) | n/a | Mega Drive or Mega-CD only | disabled,128k,256k,512k,1meg,2meg,4meg |
| gpgx | `genesis_plus_gx_add_on` | CD add-on (MD mode) (Requires Restart) | n/a | Mega Drive or Mega-CD only | auto,sega/mega cd,megasd,none |
| gpgx | `genesis_plus_gx_lock_on` | Cartridge Lock-On | n/a | Mega Drive or Mega-CD only | disabled,game genie,action replay (pro),sonic & knuckles |
| gpgx | `genesis_plus_gx_aspect_ratio` | Core-Provided Aspect Ratio | app | the app has its own picture shape setting | auto,NTSC PAR,PAL PAR,4:3,Uncorrected |
| gpgx | `genesis_plus_gx_overscan` | Borders | done | `cgear_borders` (no sides-only choice) | disabled,top/bottom,left/right,full |
| gpgx | `genesis_plus_gx_left_border` | Hide Master System Side Borders | planned |  | disabled,left border,left & right borders |
| gpgx | `genesis_plus_gx_gg_extra` | Game Gear Extended Screen | done | `cgear_gg_full` | disabled,enabled |
| gpgx | `genesis_plus_gx_blargg_ntsc_filter` | Blargg NTSC Filter | planned | TV look; Blargg's filter is LGPL, so written our own way or left to the app's shaders | disabled,monochrome,composite,svideo,rgb |
| gpgx | `genesis_plus_gx_lcd_filter` | LCD Ghosting Filter | done | `cgear_gg_ghosting` | disabled,enabled |
| gpgx | `genesis_plus_gx_render` | Interlaced Mode 2 Output | n/a | Mega Drive or Mega-CD only | single field,double field |
| gpgx | `genesis_plus_gx_frameskip` | Frameskip | app | the app has its own frame skip | disabled,auto,manual |
| gpgx | `genesis_plus_gx_frameskip_threshold` | Frameskip Threshold (%) | app | the app has its own frame skip | 15,18,21,24,27,30,33,36,39,42,... |
| gpgx | `genesis_plus_gx_ym2413` | Master System FM (YM2413) | done | `cgear_fm` | auto,disabled,enabled |
| gpgx | `genesis_plus_gx_ym2413_core` | Master System FM (YM2413) Core | planned | one exact YM2413 of our own instead of a choice of two | mame,nuked |
| gpgx | `genesis_plus_gx_ym2612` | Mega Drive/Genesis FM | n/a | Mega Drive or Mega-CD only | mame (ym2612),mame (asic ym3438),mame (enhanced ym3438),nuked (ym2612),nuked (ym3438) |
| gpgx | `genesis_plus_gx_sound_output` | Sound Output | planned |  | stereo,mono |
| gpgx | `genesis_plus_gx_audio_filter` | Audio Filter | done | `cgear_lowpass` (low-pass; no EQ yet) | disabled,low-pass,EQ |
| gpgx | `genesis_plus_gx_lowpass_range` | Low-Pass Filter % | done | `cgear_lowpass` (Light/Medium/Strong) | 5,10,15,20,25,30,35,40,45,50,... |
| gpgx | `genesis_plus_gx_psg_preamp` | PSG Preamp Level | done | `cgear_volume` | 0,5,10,15,20,25,30,35,40,45,... |
| gpgx | `genesis_plus_gx_fm_preamp` | FM Preamp Level | done | `cgear_fm_volume` | 0,5,10,15,20,25,30,35,40,45,... |
| gpgx | `genesis_plus_gx_cdda_volume` | CD-DA Volume | n/a | Mega Drive or Mega-CD only | 0,5,10,15,20,25,30,35,40,45,... |
| gpgx | `genesis_plus_gx_pcm_volume` | PCM Volume | n/a | Mega Drive or Mega-CD only | 0,5,10,15,20,25,30,35,40,45,... |
| gpgx | `genesis_plus_gx_audio_eq_low` | EQ Low | done | `cgear_eq_low` (0-150%) | 0,5,10,15,20,25,30,35,40,45,... |
| gpgx | `genesis_plus_gx_audio_eq_mid` | EQ Mid | done | `cgear_eq_mid` | 0,5,10,15,20,25,30,35,40,45,... |
| gpgx | `genesis_plus_gx_audio_eq_high` | EQ High | done | `cgear_eq_high` | 0,5,10,15,20,25,30,35,40,45,... |
| gpgx | `genesis_plus_gx_gun_input` | Light Gun Input | done | `cgear_port1` Light Phaser | lightgun,touchscreen |
| gpgx | `genesis_plus_gx_gun_cursor` | Show Light Gun Crosshair | done | `cgear_crosshair` | disabled,enabled |
| gpgx | `genesis_plus_gx_invert_mouse` | Invert Mouse Y-Axis | n/a | Mega Drive or Mega-CD only | disabled,enabled |
| gpgx | `genesis_plus_gx_no_sprite_limit` | Remove Per-Line Sprite Limit | done | `cgear_sprite_limit` | disabled,enabled |
| gpgx | `genesis_plus_gx_enhanced_vscroll` | Enhanced per-tile vertical scroll | n/a | Mega Drive or Mega-CD only | disabled,enabled |
| gpgx | `genesis_plus_gx_enhanced_vscroll_limit` | Enhanced per-tile vertical scroll limit | n/a | Mega Drive or Mega-CD only | 2,3,4,5,6,7,8,9,10,11,... |
| gpgx | `genesis_plus_gx_overclock` | CPU Speed | done | `cgear_cpu_speed` (100/150/200/300%) | 100,125,150,175,200,225,250,275,300,325,... |
| gpgx | `genesis_plus_gx_force_dtack` | System Lock-Ups | n/a | Mega Drive or Mega-CD only | enabled,disabled |
| gpgx | `genesis_plus_gx_addr_error` | 68K Address Error | n/a | Mega Drive or Mega-CD only | enabled,disabled |
| gpgx | `genesis_plus_gx_cd_latency` | CD Access Time | n/a | Mega Drive or Mega-CD only | enabled,disabled |
| gpgx | `genesis_plus_gx_cd_precache` | CD Image Cache | n/a | Mega Drive or Mega-CD only | disabled,enabled |
| gpgx | `genesis_plus_gx_show_advanced_audio_settings` | Show Advanced Audio Volume Settings (Reopen menu) | n/a | Mega Drive or Mega-CD only | enabled,disabled |
| gpgx | `genesis_plus_gx_psg_channel_0_volume` | PSG Tone Channel 0 Volume % | done | `cgear_vol_tone1` | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_psg_channel_1_volume` | PSG Tone Channel 1 Volume % | done | `cgear_vol_tone2` | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_psg_channel_2_volume` | PSG Tone Channel 2 Volume % | done | `cgear_vol_tone3` | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_psg_channel_3_volume` | PSG Noise Channel 3 Volume % | done | `cgear_vol_noise` | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_md_channel_0_volume` | Mega Drive/Genesis FM Channel 0 Volume % | n/a | Mega Drive or Mega-CD only | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_md_channel_1_volume` | Mega Drive/Genesis FM Channel 1 Volume % | n/a | Mega Drive or Mega-CD only | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_md_channel_2_volume` | Mega Drive/Genesis FM Channel 2 Volume % | n/a | Mega Drive or Mega-CD only | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_md_channel_3_volume` | Mega Drive/Genesis FM Channel 3 Volume % | n/a | Mega Drive or Mega-CD only | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_md_channel_4_volume` | Mega Drive/Genesis FM Channel 4 Volume % | n/a | Mega Drive or Mega-CD only | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_md_channel_5_volume` | Mega Drive/Genesis FM Channel 5 Volume % | n/a | Mega Drive or Mega-CD only | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_sms_fm_channel_0_volume` | Master System FM (YM2413) Channel 0 Volume % | done | `cgear_vol_fm1` | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_sms_fm_channel_1_volume` | Master System FM (YM2413) Channel 1 Volume % | done | `cgear_vol_fm2` | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_sms_fm_channel_2_volume` | Master System FM (YM2413) Channel 2 Volume % | done | `cgear_vol_fm3` | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_sms_fm_channel_3_volume` | Master System FM (YM2413) Channel 3 Volume % | done | `cgear_vol_fm4` | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_sms_fm_channel_4_volume` | Master System FM (YM2413) Channel 4 Volume % | done | `cgear_vol_fm5` | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_sms_fm_channel_5_volume` | Master System FM (YM2413) Channel 5 Volume % | done | `cgear_vol_fm6` | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_sms_fm_channel_6_volume` | Master System FM (YM2413) Channel 6 Volume % | done | `cgear_vol_fm7` | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_sms_fm_channel_7_volume` | Master System FM (YM2413) Channel 7 Volume % | done | `cgear_vol_fm8` | 0,10,20,30,40,50,60,70,80,90,... |
| gpgx | `genesis_plus_gx_sms_fm_channel_8_volume` | Master System FM (YM2413) Channel 8 Volume % | done | `cgear_vol_fm9` | 0,10,20,30,40,50,60,70,80,90,... |
| picodrive | `picodrive_region` | System Region | done | `cgear_region` + `cgear_refresh` | Auto,Japan NTSC,Japan PAL,US,Europe |
| picodrive | `picodrive_smstype` | Master System Type | planned | includes the SC-3000 (needs a keyboard) | Auto,Game Gear,Master System,SG-1000,SC-3000 |
| picodrive | `picodrive_smsmapper` | Master System ROM Mapping | done | `cgear_mapper` (Sega, Codemasters, Korean, Korean MSX, Nemesis, Janggun) | Auto,Sega,Codemasters,Korea,Korea MSX,Korea X-in-1,Korea 4-Pak,Korea Janggun,Korea Nemesis,Taiwan 8K RAM |
| picodrive | `picodrive_smstms` | Master System Palette in TMS modes | done | `cgear_tms_colours` (Auto = the real console: Master System fixed colours, SG-1000 TMS colours, Game Gear colour memory) | SMS,SG-1000 |
| picodrive | `picodrive_ramcart` | Sega CD RAM Cart | n/a | Mega Drive or Mega-CD only | disabled,enabled |
| picodrive | `picodrive_aspect` | Core-Provided Aspect Ratio | app | the app has its own picture shape setting | PAR,4/3,CRT |
| picodrive | `picodrive_ggghost` | LCD Ghosting Filter | done | `cgear_gg_ghosting` (Off/Weak/Normal) | off,weak,normal |
| picodrive | `picodrive_renderer` | Video Renderer | planned | a faster, less exact picture: only if the exact one is too slow | accurate,good,fast |
| picodrive | `picodrive_sound_rate` | Audio Sample Rate (Hz) | planned | output sample rate | 16000,22050,32000,44100,native |
| picodrive | `picodrive_fm_filter` | FM filtering | n/a | Mega Drive or Mega-CD only | off,on |
| picodrive | `picodrive_smsfm` | Master System FM Sound Unit | done | `cgear_fm` | off,on |
| picodrive | `picodrive_fmchip` | Mega Drive FM chip type | n/a | Mega Drive or Mega-CD only | ym2612,ym3438 |
| picodrive | `picodrive_audio_filter` | Audio Filter | done | `cgear_lowpass` | disabled,low-pass |
| picodrive | `picodrive_lowpass_range` | Low-Pass Filter % | done | `cgear_lowpass` | 5,10,15,20,25,30,35,40,45,50,... |
| picodrive | `picodrive_input1` | Input Device 1 | done | `cgear_port1`/`cgear_port2` (Pad, Light Phaser, Paddle, Sports Pad) | 3 button pad,6 button pad,team player,4way play,None |
| picodrive | `picodrive_input2` | Input Device 2 | n/a | Mega Drive or Mega-CD only | 3 button pad,6 button pad,None |
| picodrive | `picodrive_drc` | Dynamic Recompilers | n/a | Mega Drive or Mega-CD only | enabled,disabled |
| picodrive | `picodrive_frameskip` | Frameskip | app | the app has its own frame skip | disabled,auto,manual |
| picodrive | `picodrive_frameskip_threshold` | Frameskip Threshold (%) | app | the app has its own frame skip | 15,18,21,24,27,30,33,36,39,42,... |
| picodrive | `picodrive_sprlim` | No Sprite Limit | done | `cgear_sprite_limit` | disabled,enabled |
| picodrive | `picodrive_overclk68k` | 68K Overclock | n/a | Mega Drive or Mega-CD only | disabled,+25%,+50%,+75%,+100%,+200%,+400% |
| smsplus | `smsplus_sms_bios` | Master System BIOS (Restart Required) | planned |  | disabled,auto |
| smsplus | `smsplus_hardware` | Hardware (Restart Required) | planned | includes ColecoVision, a different console (not in S1) | auto,master system,master system II,game gear,game gear (sms compatibility),coleco |
| smsplus | `smsplus_region` | Region (Restart Required) | done | `cgear_region` | auto,ntsc-u,pal,ntsc-j |
| smsplus | `smsplus_fm_sound` | Use FM (YM2413) (Restart Required) | done | `cgear_fm` | auto,disabled |
| smsplus | `smsplus_hide_left_border` | Hide SMS Left Border | planned |  | disabled,enabled |
| smsplus | `smsplus_ntsc_filter` | Blargg NTSC Filter | planned | as above | disabled,composite,svideo,rgb,monochrome |
