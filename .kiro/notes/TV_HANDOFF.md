# Handoff: Continuum for the owner's LG TV

Paste this to start a new chat about the TV version:

> Continue Continuum (repo 2c2bhdhw9z-cell/Continuum-), the LG TV version. Read
> `.kiro/steering/owner-rules.md`, then `.kiro/notes/TV_HANDOFF.md`, then ask me the open
> questions in it before building anything. My TV's SSH key and today's connection address are
> below.
> (then paste the `-----BEGIN OPENSSH PRIVATE KEY-----` block and the Pinggy `tcp://...` line)

The SSH key is never committed to this repo. The owner pastes it into each chat.

Written 22 November 2026, 5 PM Eastern, at the end of the chat that rooted-debloated the TV and
built its launcher. Everything below was checked on the TV itself in that chat.

## What the owner wants

- **A Continuum app for the TV that is super light**, about as light as RetroArch, but easy to
  use with the remote and a controller. RetroArch is installed on the TV, and the owner finds it
  "so confusing to navigate".
- **The iPhone app must not change at all.** The TV app is separate and must not alter how the
  phone app builds or behaves.
- **Systems:** Game Boy, Game Boy Color, Game Boy Advance, SNES and DS. N64 and 3DS only if the
  TV can manage them (owner: "If 3ds or n64 is to much that's fine just Let me know what it can
  all run"). **No PS1**, nothing heavy ("it's obviously a TV").
- **ONLY THE OWNER'S OWN CORES. Nobody else's emulators, ever** (owner, 22 November 2026:
  "It needs to be my cores, nobody else's"). That means the cores written from scratch in this
  repo under `cores/`. It does not mean mGBA, bsnes, Mesen, melonDS, ParaLLEl N64, Azahar,
  Gearsystem or any other emulator the phone app bundles. RetroArch's downloadable cores are out
  too, even for tests. A system with no core of the owner's own is not in the TV app.

## The owner's own cores, and what this TV can run

| Own core | Folder | Systems | State (from STATUS.md and tasks/, 22 November 2026) | On this TV |
|---|---|---|---|---|
| **cBoy** | `cores/continuum-gb` | Game Boy, Game Boy Color, Game Boy Advance | The most finished. All Mooneye Game Boy and Color tests pass, and full Pokémon Yellow played on a phone. Advance passes 95 of Alyosha's 133 timing tests (129 is the ceiling; mGBA passes 39). Next: `tasks/cboy-finish.json` | Yes. Start here |
| **cGear** | `cores/continuum-sms` | Master System, Game Gear (SG-1000: check) | In the app, with cheat codes | Yes, light |
| **cAtari** | `cores/continuum-atari` | Atari 7800 | Finished 27 October, optional in Settings | Yes, light |
| **cMini** | `cores/continuum-mini` | Pokémon Mini | Built, not tested on a phone | Yes, light |
| **cArcade** | `cores/continuum-arcade` | Arcade: Pac-Man, Galaxian, Scramble, Frogger, Donkey Kong and their clones | Brand new, barely tested | Yes, light |
| GameCube | `cores/continuum-gc` | GameCube | Barely started (`tasks/gamecube.json`) | No, far too heavy |

The owner also wants **SNES and DS**, and maybe N64 and 3DS. None of them has an own core yet,
so the TV app starts without them. Ask before starting a new own core.
- **SNES:** a realistic next own core for the TV.
- **DS:** a big job, and it would have to be light enough for this chip.
- **N64 and 3DS:** too heavy for this TV. Tell the owner plainly.

Speeds above are estimates. The chip is a budget quad-core TV processor, several times slower
than a recent iPhone, and runs 32-bit ARM code only. Measure each core on the TV before telling
the owner a system works: frame rate in a demanding part of a real game, and memory in use.
- The owner's general rules (`.kiro/steering/owner-rules.md`) apply here too: plain English,
  ask before building, everything customisable, push to master, and never start an iPhone build
  without a yes.

## Open questions to ask the owner first (short, numbered, with choices)

1. ANSWERED (22 November 2026): **the first TV version has cBoy only** (Game Boy, Game Boy
   Color, Game Boy Advance). Owner: "Cboy since idr play any of the other ones that would be
   playable on the tv." Don't add cGear, cAtari, cMini or cArcade unless he asks.
2. Should a new own SNES core be started for the TV (and later DS), and before or after the TV
   app works?
3. Should it look like the phone's New look (swirling background etc.) or a simpler TV layout?
   Every effect needs its own switch either way (owner rule).
4. Where should games come from? Today they arrive through the phone upload page into
   `/media/developer/games` (see below). Keep that, add Continuum's own transfer, or both?
5. Save files and save states: TV only, or shared with the phone later?
6. Should it replace the TV's Home screen launcher, or be an app on it? (Recommendation: an app
   on the existing Kiro launcher, which already works.)

## The TV (facts measured on it)

| | |
|---|---|
| Model | LG 55UQ7570PUJ (2022, US) |
| webOS | webOS 22, SDK 7.6.0, firmware 04.60.85 |
| Processor | quad-core ARM (LG α5 Gen 5). Kernel is 64-bit, **all of userspace is 32-bit ARM** (armv7 hard-float, glibc). Build everything for `armv7-unknown-linux-gnueabihf`. |
| RAM | 1.5 GB nominal, 1236 MB visible, typically **about 200 MB free** with apps loaded |
| Screen | 4K panel. Apps draw at **1920x1080** (the TV scales it up); video plays on a separate 4K plane |
| Graphics | OpenGL ES through Wayland. No Vulkan. |
| Storage | `/media/developer` shares a partition with about **1.1 GB free** |
| Node.js | 12.21 (for small helper scripts only) |
| Network | Wi-Fi, LAN address 192.168.12.50 |
| Root | Homebrew Channel, root SSH (dropbear) with the owner's key, failsafe mode on |
| Firmware updates | blocked by the Homebrew Channel (keeps root). Do not unblock. |

### How RetroArch runs here (the model for a light native app)

- Installed from the Homebrew Channel as `com.retroarch.webos` (1.22.2), a **native** app:
  `appinfo.json` has `"type": "native"`, `"main": "retroarch"`, plus `"cloudgame_active": true`,
  `"useAllMouseButtons": true` and `"useAllKeyboardKeys": true` (so the remote's keys reach it).
- It ships its own `lib/libSDL2-2.0.so.0` (a webOS build of SDL2), and uses SDL2 for input,
  GL for video and PulseAudio for sound (`video_driver = "gl"`, `input_joypad_driver = "sdl2"`,
  `audio_driver = "pulse"`).
- Its HOME is its own app folder, with config at `.config/retroarch/retroarch.cfg`. Anything in
  the app folder is lost when the app is updated or reinstalled.
- RetroArch downloads other people's cores from `http://retroarch-cores.webosbrew.org/armv7a/`.
  **Those are not for Continuum** (owner rule above). What does carry over is the format: the
  owner's own cores are already libretro `cdylib`s in Rust, so built for armv7 Linux they load the
  same way a RetroArch core does.
- A good reference for a light native webOS game app is Moonlight TV (mariotaku/moonlight-tv):
  native, SDL2, controller and remote support. It was not checked in this chat, so verify first.

### Controllers and the remote

- The owner has an Xbox controller. webOS 22 pairs it over Bluetooth (Settings, General, Devices,
  External Devices, Bluetooth Controller; not tested in this chat). SDL2 should see it as a
  gamepad.
- The remote is LG's plain IR remote (AKB-style: number pad, Netflix, Prime, Disney+,
  LG Channels and four colour buttons). It shows as `/dev/input/event1` "LGE RCU". In web pages,
  OK = keyCode 13, Back = 461 and Red = 403. The Linux Home key code is 773.
- The **Prime Video button is remapped** to open YouTube AdFree (LG Input Hook, keybind 1038 in
  `/home/root/.config/lginputhook/keybinds.json`). Don't let a TV app grab that button.

## What is already on the TV (don't break it)

All of it was set up in that chat. It lives in `/media/developer/kiro` plus the boot hooks, and
can all be undone. A copy of every script is in this repo at `tools/lg-tv-kit/`.

- **Debloat** (`/media/developer/kiro/kiro-debloat.sh`, boot hook `40-kiro-debloat`):
  - LG ads, promotions and recommendations are off, and so are Live Plus (ACR), the ad cookie
    and debug log uploads.
  - LG's ad and ACR servers are blocked in `/etc/hosts`. That's a RAM copy, rebuilt at boot.
  - LG's suggestion and telemetry services, LG ThinQ (`iot-client`) and Matter (`chip-service`)
    are stopped at every boot.
  - The preloader no longer keeps LG Channels, the browser, the voice app or Prime Video in
    memory.
  - Hidden through `deletedSystemAppList.json`: LG Channels, Sports Alert, Alexa, Room to Room
    Share, store demos, Web Browser and Media Player.
  - Uninstalled: ShopTime, LG Movies & TV, LG Radio+, LG Travel+, Weekend Games, 8 Ball Pool,
    Gaming Portal, Sports Playbook, LG Fitness, TV Cast, Sling, YouTube TV, Fubo, Peacock,
    Apple TV, Amazon Luna, Prime Video, and LG's official YouTube.
  - Backups are in `/media/developer/kiro/backup`. `sh kiro-debloat.sh restore` puts it all back.
- **YouTube AdFree** (webosbrew youtube-webos 0.5.3) is installed in place of LG's YouTube, with
  the same app ID `youtube.leanback.v4`. That ID is on SAM's removed list, which stops LG's own
  YouTube coming back; the Homebrew copy still runs. If LG's YouTube ever reappears as a "stub",
  remove it through `appInstallService/remove`, then reinstall AdFree from the Homebrew Channel.
- **Kiro launcher**, the TV's Home screen (`/media/developer/kiro/home`, boot hook `50-kiro-home`):
  - It's a plain HTML/CSS/JS page. LG's own Home program is bind-mounted over **in RAM only**
    by a small stand-in script, so SAM starts the page as `com.webos.app.home`. SAM is never
    restarted, because restarting it breaks HDMI picture and ARC sound on these TVs.
  - LG's Home is restored from a CRIU snapshot on this TV, and the stand-in still works with that.
  - A local helper (`kiro-api.sh` under BusyBox tcpsvd, `127.0.0.1:8766` only) does the Luna
    calls the page itself may not make: launching apps, listing inputs and so on.
  - The top bar has Inputs, Backgrounds, Upload, Settings, App Store, Homebrew and LG Home
    (LG Home is a temporary escape until the next reboot).
  - The Play row has Xbox (HDMI 1) and AirPlay. HDMI 2 is the soundbar's ARC connection and is
    hidden. The apps grid follows. A new TV app shows up in the grid by itself; to give it a
    fixed place, add its ID to `ORDER` in `app/main.js`.
  - After a cold boot the TV shows HDMI 1 for about 45-50 seconds, until the Homebrew boot hooks
    run. Normal power off and on uses Quick Start+, which is on.
- **Backgrounds** (`kiro-bg.sh`, files in `home/app/bg`):
  - One picture, shuffle with an interval, or none.
  - The screensaver can show the owner's pictures, dimmed with a clock. That works by
    bind-mounting `Slideshow.qml` over LG's `ScreenSaver.qml` and `Clock.qml`, in RAM only.
  - The red button pressed twice deletes a picture.
- **Phone upload page** (`upload.js`, started from the launcher's Upload button):
  - It listens on port 8767 under a random 5-letter code and stops 15 minutes after its last
    use.
  - Pictures are resized on the phone to 1920x1080 and go to the backgrounds.
  - **Game files go to `/media/developer/games`** as they are (up to 1 GB each, always keeping
    300 MB free). RetroArch's Load Content starts in that folder: `rgui_browser_directory` was
    changed, and the original config is backed up.

## How to work on the TV from a chat

- The owner opens a tunnel on their iPhone (it expires after **60 minutes**):
  `ssh -p 443 -o ServerAliveInterval=30 -R0:192.168.12.50:22 tcp@free.pinggy.io`
  and pastes the `tcp://<name>.run.pinggy-free.link:<port>` line.
- In this sandbox, **`/tmp` does not survive between commands** and the home folder is
  read-only. So write the key and run `ssh` in the **same** command: `printf` the key lines into
  `/tmp/tv_key`, `chmod 600`, then
  `ssh -q -i /tmp/tv_key -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ServerAliveInterval=15 -o BatchMode=yes -p <port> root@<name>.run.pinggy-free.link '...'`.
  Copy files with `tar -cf - ... | ssh ... 'tar -C <dir> -xf -'`.
- **luna-send** on 7.6.0 can drop replies when its stdin closes. Use `lsend` from
  `/media/developer/kiro/lib.sh`, which holds stdin open on a FIFO. Its `fgapp` prints the app in
  front.
- **Screenshots:**
  `lsend -n 1 -w 10000 luna://com.webos.service.capture/executeOneShot '{"path":"/tmp/s.jpg","method":"DISPLAY","format":"JPEG","width":960,"height":540}'`,
  then `cat` the file back and read it as an image.
- **Pressing remote buttons from a script:**
  `node /media/developer/kiro/tools/press.js /dev/input/event1 up down left right enter back lghome wait1000`.
  It writes 16-byte input events, the 32-bit layout this TV uses.
- **Installing a package:**
  `luna-send -i luna://org.webosbrew.hbchannel.service/install '{"ipkUrl":"...","ipkHash":"<sha256>"}'`.
  A package with a Luna service needs
  `/media/developer/apps/usr/palm/services/org.webosbrew.hbchannel.service/elevate-service <service id>`
  afterwards.
- **Rebooting** (asks first, owner rule):
  `luna-send -n 1 luna://com.webos.service.sleep/shutdown/machineReboot '{"reason":"remoteKey"}'`.
  Start it detached with `setsid` and wait about 80 seconds.
- **Never write** to `/usr`, the kernel, rootfs or tvservice (bricks the TV). Keep changes in
  `/media/developer`, `/var/lib/webosbrew/init.d`, `/var/preferences`, and back up before editing.
  Never touch `/home/root/.ssh/authorized_keys`.

## Suggested shape for the TV app (decide with the owner first)

- **Where in the repo:** a new top-level folder, for example `tv/webos/`. **Not under `native/`
  and not changes in `crates/`**: on master, a push to `native/**` or `crates/**` starts the
  iPhone build (`ios.yml`), and the owner must say yes to every build. If the shared engine needs
  a change for the TV, say so and ask first.
- **The engine:** `crates/emulator-bridge` already runs libretro cores and has the import code
  (zip, 7z, CHD), saves, cheats and so on. Its graphics side uses wgpu with Metal and Vulkan,
  which doesn't fit this TV (GLES only). Check whether the non-graphics parts build for
  `armv7-unknown-linux-gnueabihf` as they are. If they don't, the TV app is a small Rust program
  of its own: SDL2 for the window, GLES, sound and controllers; `libloading` for the cores; and a
  simple remote-friendly menu drawn in GL.
- **Light means native.** A native SDL2 program has no browser engine. The launcher's web page
  alone costs WebAppMgr 30-45 MB. Target well under 100 MB in use while a game runs.
- **Building for the TV:**
  - The sandbox has `rustc` and `cargo` (the repo pins the toolchain in `rust-toolchain.toml`)
    and `clang`. The armv7 target and a sysroot still have to be set up.
  - For the sysroot, use either the webOS NDK (openlgtv/buildroot-nc4, `arm-webos-linux-gnueabi`;
    verify that name) or the TV's own `/lib` and `/usr/lib` copied over SSH, so glibc matches
    exactly.
  - Link SDL2 against RetroArch's webOS `libSDL2-2.0.so.0`, or the same webOS SDL2 build, and
    ship it in the app's `lib/` with an rpath of `$ORIGIN/lib`.
- **Packaging:**
  - An `.ipk` holding `appinfo.json` (`"type": "native"`, plus the keyboard and mouse flags
    RetroArch uses), the binary, `lib/` and the icons.
  - Publish it on GitHub Releases, and optionally as a Homebrew Channel repository
    (`apps.json`), so the owner installs and updates it from the TV.
- **Cores:** only the owner's own, from `cores/` (cBoy first), built with
  `cargo build --release --target armv7-unknown-linux-gnueabihf` in each core's folder.
  - They are pure Rust `cdylib`s, so no other emulator's C or C++ build is needed.
  - Don't touch `scripts/build-core.sh`; it is the iOS core recipe and cache.
  - A push to `cores/continuum-gb/**` starts the iPhone build. Put TV-only build files under
    `tv/` rather than in the core's folder.
  - Ship the cores inside the `.ipk`.
- **Games and saves:** read `/media/developer/games` (what the upload page fills). Keep saves and
  the app's own data outside its app folder (for example `/media/developer/continuum`) so an
  update never deletes them.
