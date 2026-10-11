# cBoy settings inventory

The rule (docs/CORE_OPTIONS_STANDARD.md §0): cBoy offers the union of every setting the notable
Game Boy, Game Boy Color and Game Boy Advance emulators offer. If even one of them has it, cBoy
has it, or this table says plainly why not and where it lives instead.

Emulators surveyed: mGBA, Gambatte, SameBoy, VBA-M, gpSP, Emulicious, BGB, NanoBoyAdvance
(libretro core options and their standalone settings). Columns: which of them offer the setting,
and cBoy's key. Everything in cBoy is our own implementation and wording; no code, keys or text
were taken from any of them.

Key: **Yes** = cBoy has it. **App** = provided by Continuum for every core (not a core option, by
the standard §6). **Gap** = not done yet, with the reason.

## System

| Setting | Offered by | cBoy |
|---|---|---|
| Console / model choice (DMG, Pocket, SGB, SGB2, CGB, AGB) | mGBA, Gambatte, SameBoy, VBA-M, BGB, Emulicious | Yes `cboy_gb_model` |
| Auto model from the cartridge | all | Yes (`auto`) |
| Real boot ROM / BIOS from a file | mGBA, Gambatte, SameBoy, VBA-M, gpSP, BGB, Emulicious, NanoBoyAdvance | Yes `cboy_boot_rom`, `cboy_gba_bios` |
| Skip the BIOS logo while using a real BIOS | mGBA, gpSP, NanoBoyAdvance | Yes `cboy_gba_bios=file_skip` |
| Built-in BIOS replacement (no file needed) | mGBA, gpSP, NanoBoyAdvance | Yes (built-in HLE BIOS, default) |
| GBA save chip override (SRAM, Flash 64/128, EEPROM 512/8K, none) | mGBA, VBA-M, gpSP, NanoBoyAdvance | Yes `cboy_gba_save_type` |
| Force cartridge hardware (RTC, solar, gyro, tilt, rumble) | mGBA, VBA-M, NanoBoyAdvance | Yes `cboy_gba_hardware` |
| Cartridge clock: real time / game time / host clock | SameBoy, VBA-M, mGBA, BGB | Yes `cboy_rtc` |
| Clock offset | mGBA, BGB | Yes `cboy_rtc_offset` |
| CGB hardware revisions (0, A-E) | SameBoy | Gap: revisions differ only in hardware bugs cBoy doesn't model; one CGB is emulated |
| SGB timing, NTSC and PAL (SNES clock) | SameBoy | Yes `cboy_gb_model=sgb` / `sgb_pal` |
| Game Boy Player detection (rumble in GBA games) | mGBA, VBA-M, gpSP | Gap: needs the GB Player logo/serial handshake; planned |
| Built-in free boot ROMs (animated logo without a file) | SameBoy | Gap: we start bootless instead; writing our own boot ROMs is planned |
| Dynamic recompiler on/off | gpSP | Not applicable: iOS forbids JIT for sideloaded apps; cBoy is an interpreter |

## Picture

| Setting | Offered by | cBoy |
|---|---|---|
| DMG palette presets | mGBA, Gambatte, SameBoy, VBA-M, BGB, Emulicious | Yes `cboy_dmg_palette` (7 presets) |
| Custom palette | mGBA, Gambatte, VBA-M, BGB, Emulicious | Yes (`custom`, palette.txt) |
| Colourise DMG games (separate BG/sprite colours, auto by title) | Gambatte, mGBA, SameBoy, VBA-M | Yes `cboy_dmg_colorize` (own schemes; Nintendo's CGB boot table not copied) |
| GBC colour correction (several modes) | Gambatte, SameBoy, mGBA, VBA-M, BGB | Yes `cboy_gbc_color` |
| GBA colour correction / LCD look (several modes) | mGBA, gpSP, VBA-M, NanoBoyAdvance | Yes `cboy_gba_color` (Advance, SP, TV) |
| Colour temperature | SameBoy | Yes `cboy_color_temp` |
| Contrast boost / reduce | SameBoy | Yes `cboy_contrast` |
| Dark filter | Gambatte | Yes `cboy_dark_filter` |
| Frame blending / LCD ghosting | mGBA, Gambatte, SameBoy, VBA-M, gpSP, BGB, NanoBoyAdvance | Yes `cboy_blend` |
| SGB border on / off / always | mGBA, SameBoy, VBA-M | Yes `cboy_sgb_border` |
| Layer toggles: GB BG, window, sprites | VBA-M, mGBA, Emulicious, BGB | Yes `cboy_layer_bg/window/obj` |
| Layer toggles: GBA BG0-3, OBJ, WIN0, WIN1, OBJ window | VBA-M, mGBA | Yes `cboy_layer_bg0..3`, `cboy_layer_obj`, `cboy_layer_win0/win1/objwin` |
| Sprite limit off | mGBA, Emulicious, BGB | Yes `cboy_sprite_limit` |
| Scaling, aspect, shaders, integer scale, video filters | all | App |

## Sound

| Setting | Offered by | cBoy |
|---|---|---|
| Master volume | most | Yes `cboy_volume` |
| Per-channel volume / mute (4 PSG + 2 DirectSound) | VBA-M, mGBA, BGB, Emulicious, SameBoy | Yes `cboy_vol_*` |
| Low-pass filter | mGBA, VBA-M | Yes `cboy_lowpass` |
| High-pass / capacitor modes | SameBoy | Yes `cboy_highpass` |
| Sample interpolation / resampler quality | Gambatte, VBA-M, NanoBoyAdvance | Yes `cboy_audio_quality`, `cboy_gba_smooth` |
| Output sample rate | BGB, Emulicious, NanoBoyAdvance | Yes `cboy_sample_rate` |
| MP2K (Nintendo sound driver) high-quality HLE mixing | NanoBoyAdvance | Gap: a large, separate job (recognise the driver, re-mix at high rate); planned |
| Audio interference sound | SameBoy | Yes `cboy_interference` |
| Audio latency / buffer | most | App |

## Controls

| Setting | Offered by | cBoy |
|---|---|---|
| Turbo buttons and rate | Gambatte, VBA-M, gpSP, mGBA | Yes `cboy_turbo` |
| Rumble on/off and strength | Gambatte, SameBoy, mGBA, VBA-M, gpSP | Yes `cboy_rumble` |
| Tilt (MBC7, GBA tilt) source | mGBA, VBA-M, SameBoy | Yes `cboy_tilt_source` |
| Gyro source | mGBA, VBA-M | Yes `cboy_gyro_source` |
| Sensor sensitivity | mGBA, VBA-M | Yes `cboy_motion_sensitivity` |
| Solar level / light sensor | mGBA, VBA-M, NanoBoyAdvance | Yes `cboy_solar` |
| Allow opposite directions | mGBA, Gambatte | Yes `cboy_opposing` |
| Analog stick as D-pad | Gambatte | Yes `cboy_stick_dpad` |
| Button remapping, on-screen controls | all | App |

## Link

| Setting | Offered by | cBoy |
|---|---|---|
| Link cable (local loopback / test) | BGB, Emulicious | Yes `cboy_link` |
| Network link between two devices | Gambatte, BGB, SameBoy (two screens), gpSP | Gap: placeholder; needs Continuum's netplay to carry serial bytes |

## Performance

| Setting | Offered by | cBoy |
|---|---|---|
| Frame skip (fixed) | mGBA, VBA-M, gpSP, BGB | Yes `cboy_frameskip` |
| Frame skip (auto) | mGBA, gpSP | Yes `cboy_frameskip=auto` (by the core's own frame time; Continuum doesn't report audio-buffer level to cores) |
| Run-ahead / input latency | RetroArch frontend for all | Yes, in-core `cboy_run_ahead` |
| Idle-loop detection / skipping | mGBA, gpSP | Yes `cboy_idle_skip` (Advance) |
| Lower-cost audio | Gambatte, VBA-M | Yes (`cboy_audio_quality=fast`, `cboy_sample_rate`) |
| Fast-forward speed | NanoBoyAdvance, BGB, mGBA standalone | App (Settings > Speed and rewind) |
| Rewind | mGBA standalone, NanoBoyAdvance | App (Settings > Speed and rewind) |
| Low-accuracy fast mode | none of the above as one switch | Not offered: no setting to match; the speed options above cover it |

## Debugger-only features (Emulicious, BGB, mGBA, SameBoy)

Memory viewers, breakpoints, VRAM/tile viewers and exception-on-bad-access settings are developer
tools, not play settings. They are out of scope for a core option; cBoy logs lockups and CGB DMA
events to the activity log instead.

## Cheats, save states, screenshots

App (every core).
