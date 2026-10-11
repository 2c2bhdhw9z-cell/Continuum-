# Core options standard for Continuum's own cores

Every core Continuum writes itself (cBoy, Continuum Symbian, and any later one) declares its
settings the same way, so the app's Core Settings sheet looks and behaves the same for all of
them and nothing has to be special-cased in Swift.

## 0. The rule: the union of every notable emulator

A Continuum core offers every setting that ANY notable emulator for that system offers. If only
one of them has a setting, our core has it too. Before a core ships, its folder in `docs/` has an
inventory table: each setting, which emulators offer it, and whether our core has it (key), the
app provides it (§6), or it is a gap with a stated reason. Gaps are closed over time; "nobody
asked" is not a reason. This applies to every future Continuum core (DS, N64, and so on). cBoy's
table is `docs/CBOY_OPTIONS_INVENTORY.md`.

Feature parity is the goal, not copying: implement every setting ourselves, with our own keys,
wording and code.

## 1. How options are declared

- Use `RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2` (67), called from `retro_set_environment`, so the
  frontend knows the options before a game loads. If it returns false, fall back to
  `RETRO_ENVIRONMENT_SET_VARIABLES` (16) with the same keys and the default listed first.
- Read values with `RETRO_ENVIRONMENT_GET_VARIABLE` at `retro_load_game`, and again whenever
  `RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE` reports a change (checked once per `retro_run`).
- Declared tables are built once and never freed: the frontend may keep the pointers.

## 2. Keys

- Prefix: the core's short name and an underscore. cBoy uses `cboy_`, Continuum Symbian uses
  `csymbian_`. Never reuse another emulator's keys or wording.
- After the prefix: lower-case words joined by `_`, naming the thing, not the UI
  (`cboy_dmg_palette`, not `cboy_palette_picker`).
- Values: short lower-case tokens (`off`, `on`, `auto`, `grayscale`, `50`). On/off options use
  exactly `off` and `on`. Numbers are plain decimal, units in the label (`50` → "50%").
- A key, once shipped, is never renamed or given a different meaning; retire it instead
  (users' saved choices are stored under the key).

## 3. Categories (options v2 categories)

Every option belongs to one category. Use these keys and only add a new one when none fits:

| key        | shown as  | what goes there |
|------------|-----------|-----------------|
| `system`   | System    | which machine or model, BIOS / boot ROM, clock, save type |
| `video`    | Picture   | palettes, colour correction, frame blending, borders, layer toggles |
| `audio`    | Sound     | volume, channel mix, filters, interpolation |
| `input`    | Controls  | turbo, sensors, rumble — things the cartridge or console has |
| `link`     | Link      | link cable, multiplayer adapters |
| `performance` | Performance | frame skip, in-core run-ahead, idle-loop skipping, cheaper audio |
| `debug`    | Debug     | layer toggles and anything only a developer needs |

Descriptions: the label is short ("Game Boy palette"); the info line says in one plain sentence
what changes and when it applies ("Applies right away." / "Takes effect on the next restart.").

## 4. Defaults

- The default is what the real hardware does, unmodified: no colour correction, no blending,
  full volume, the model the cartridge asks for, the built-in boot path.
- The default value is listed first in the value list and named in `default_value`.
- An option whose feature needs a file (a BIOS, a custom palette) defaults to not needing it, and
  says in its info line where the file goes (`<system folder>/<core>/...`).

## 5. Live versus restart

- Apply live whenever possible (picture, sound, turbo, layers, sensors).
- Options that change the machine (model, boot ROM, save type, cartridge hardware) apply on the
  next game start or reset; their info line says so.
- Options never change what is stored in a save state or battery save. A state taken with one
  setting loads with another.

## 6. What belongs in the core, what belongs in the app

In the core (a core option): anything the emulated hardware would have had, or how its own
output is produced: models, BIOS, palettes of the machine, colour of its LCD, LCD ghosting,
the cartridge's clock, sensors and rumble, save chip type, channel volumes, link cable.

In the app (never a core option): skins, scaling and aspect, screen filters and shaders,
on-screen controls and button mapping, fast-forward and its speed, rewind, save-state slots,
cheats, the frontend's own audio latency. If the app already offers it for every core, the core
must not duplicate it. Continuum has fast-forward (2x-4x) and rewind in Settings > Speed and
rewind, for every core.

Performance settings that only the core can do (frame skip that skips drawing, run-ahead using
the core's own states, idle-loop skipping, a cheaper sound path) are core options in the
`performance` category. Automatic frame skip by audio-buffer level needs the app to report its
buffer to the core, which Continuum does not do yet.

Continuum Symbian currently declares no options (its engine exposes none yet). When it gains
any, they use the `csymbian_` prefix and these categories.

## 7. Testing

Every option has a test in the core's own test suite that sets it and checks its effect
(a frame, a sample, a register), and the frontend-path test checks the table is declared.
