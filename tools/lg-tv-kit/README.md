# LG TV kit

Copies of the scripts running on the owner's rooted LG 55UQ7570PUJ (webOS 22): the debloat, the
Kiro Home screen launcher, backgrounds with the picture screensaver, and the phone upload page for
pictures and games. On the TV they live in `/media/developer/kiro`. What each one does, and how to
work on the TV from a chat, is in `.kiro/notes/TV_HANDOFF.md`.

- `tv/`: `kiro-debloat.sh` (status, apply, restore), its boot hook, and shared helpers (`lib.sh`, `kj.js`)
- `launcher/`: the launcher page (`app/`), the Home stand-in, the local helper, backgrounds,
  the screensaver page and the upload page
- `tools/press.js`: presses remote buttons from a script, for testing

Not part of the iPhone app. Nothing here is built by the workflows.
