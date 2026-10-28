# cMini: Pokémon Mini settings inventory

Compared on 28 October 2026 with PokeMini libretro's 13 current settings. PokeMini stays the default. cMini uses the `cmini_` prefix and gives every setting a real effect.

| PokeMini setting | cMini | Effect |
|---|---|---|
| Video Scale | `cmini_video_scale` | Draws the 96×64 LCD at 1× through 6× before the app fits it. |
| 60Hz Mode | `cmini_60hz` | Drops each sixth 72 Hz picture, while machine and sound timing remain native. |
| LCD Filter | `cmini_lcd_filter` | None, scanlines, or visible dot-cell gaps. |
| LCD Mode | `cmini_lcd_mode` | Analog persistence, three shades, or two shades. |
| LCD Contrast | `cmini_contrast` | Changes the dark/light separation. |
| LCD Brightness | `cmini_brightness` | Moves the whole LCD range lighter or darker. |
| Palette | `cmini_palette` | Original yellow-green, monochrome, green, blue, red, sepia, or pink LCD. |
| Piezo Filter | `cmini_piezo` | Applies the measured AC-coupled piezo response. |
| Low Pass Filter | `cmini_lowpass` | Switches high-frequency softening on. |
| Low Pass Level | `cmini_lowpass_level` | Changes the filter cutoff. |
| Screen Shake | `cmini_shake` | Sets the amount associated with the hardware motor signal. |
| Controller Rumble | `cmini_rumble` | Scales the hardware motor signal sent through libretro rumble. |
| Turbo Button Period | `cmini_turbo` | Changes the Turbo A pulse period. |

## Hardware-backed extras

- `cmini_bios`: starts through the player's exact 4 KiB `bios.min`; off starts the cartridge directly at its documented entry point. cMini contains no replacement logo or made-up startup picture.
- `cmini_infrared`: connects the real infrared transmit/receive register bits to Continuum's link transport. With no peer, the receiver stays idle.

The app already owns screen sizing, screen effects, fast-forward, rewind, controller remapping, achievements, and save-state slots, so cMini does not duplicate those controls.
