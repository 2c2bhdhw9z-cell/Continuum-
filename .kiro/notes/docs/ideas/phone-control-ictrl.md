# Idea: let Kiro use the phone itself (iCTRL)

The owner raised this on 25 October 2026, about 6:15 PM Eastern, with Gemini screenshots. At about
6:35 PM he said to save it as a possible idea for later. He tests build 233 first.
**Nothing here is built. Do not start it unless the owner asks.**

## What it would give

iCTRL (github.com/OpenMinis/iCTRL, Apache-2.0, release 1.98.12 of 15 October 2026) runs on the iPhone
and lets an agent tap, swipe, type, read the screen, take screenshots and open apps. Kiro could then
open Continuum on the owner's phone, go through menus and games, and find problems without asking
him to test. Kiro still cannot hear sound; smoothness still comes from the diagnostic files.

## Time it takes

About an hour the first time, 30 to 40 minutes of it the owner's tapping. After that, about 2 to 3
minutes each time he wants Kiro on the phone. Everything is free.

## What the setup needs (checked in iCTRL's own README on 25 October, not only Gemini's summary)

1. **Signing.** The app (`iCTRLUITests-Runner.ipa`) has a test part inside it,
   `PlugIns/iCTRLUITests.xctest`, that must be signed first, with the runner's bundle id. Feather and
   ESign skip that part, and the app then crashes the moment it opens (the README warns about this).
   Two ways that work: the latest SideStore (free Apple ID), or iCTRL's `Scripts/sign-ipa.sh`, which
   uses Apple's `codesign` and so needs a Mac: a GitHub Actions job on a macOS runner.
   - For the GitHub job, the owner adds three repository secrets from his CertDrop development
     certificate: the .p12 (as base64 text, e.g. `IOS_P12_BASE64`), its password
     (`IOS_P12_PASSWORD`) and the development .mobileprovision (as base64, `IOS_PROVISION_BASE64`).
     The Shortcuts app can turn a file into base64 text on the phone. Never put them in the repo.
   - The job downloads the release's unsigned runner .ipa (no need to build iCTRL), imports the
     certificate into a temporary keychain, installs the profile, runs
     `Scripts/sign-ipa.sh -i iCTRLUITests-Runner.ipa`, and publishes the signed .ipa as a Release
     asset (a direct link the phone can open; a workflow artifact downloads as a zip).
   - **Open question:** the profile has to cover the runner's bundle id. A wildcard (`TEAMID.*`)
     profile does; if his CertDrop profile is for one exact id, the runner's id has to be changed to
     match first. Check the profile before writing the job.
   - The owner installs the signed .ipa with ESign **without** signing it again.
2. **Developer Disk Image.** The runner needs it mounted. StikDebug, which he already uses for JIT,
   mounts it at launch. A restart of the phone unmounts it: open StikDebug again.
3. **First launch.** Tap the runner on the Home Screen. Allow Notifications (the runner is
   controlled from its notification) and Local Network; iOS may ask for the passcode.
4. **Reaching it from Kiro's cloud sandbox.** The runner listens on 127.0.0.1:8200 on the phone. A
   free terminal app on the same phone carries it out, with no second VPN, so LocalDevVPN and JIT
   stay on:
   - Install iSH from the App Store, then `apk add openssh-client`.
   - `cat /dev/location > /dev/null &` keeps iSH running in the background (asks for location; costs
     some battery).
   - `ssh -R 80:127.0.0.1:8200 nokey@localhost.run` prints a public https link (no account). The link
     changes every time. The sandbox reached localhost.run when checked on 25 October.
   - Because the tunnel starts on the phone itself, iCTRL's "Remote Access" (Wi-Fi) switch should not
     be needed; check that on the first try.
5. **Each session.** LocalDevVPN on, start the runner, start the iSH command, paste the link to Kiro,
   approve Kiro's pairing request on the phone (a new sandbox has a new key, so expect one approval
   each session). The phone stays unlocked while Kiro works. Turn it all off afterwards.

## On Kiro's side

`GET /skill` (the on-device manual) and `GET /schema` (every method), download the client the phone
serves, pair (X25519, approved on the phone; every request after that is encrypted and signed), then
JSON-RPC calls. Rules for Kiro while driving: only open Continuum; never kill the app while JIT is on
(it would lose JIT; check what is on screen instead); stop when done.

## Safety, in plain words for the owner

Anyone could find the link, but nothing works until he approves the request on his phone, and
everything sent is scrambled. While it is on, it can control the whole phone, so it should be turned
off when the session ends.

## The owner's own signer could replace the GitHub job (checked 25 October, about 6:55 PM Eastern)

The owner is writing his own on-device signer, Signer (repo 2c2bhdhw9z-cell/snes-tas-studio, its own
engine, not zsign); he has installed Continuum with it. Signer as it is now was run here on iCTRL's real
runner (v1.98.12) with Signer's own stand-in test certificate (one-ID profile, get-task-allow true):
- it re-signed the inner `PlugIns/iCTRLUITests.xctest` with the certificate's team and the SAME app ID
  as the runner (the release ships both as com.openminis.ictrl.uitests.xctrunner; with a one-ID profile
  Signer moves both to the profile's ID together), which is exactly what ESign and Feather fail to do;
- it signed `Frameworks/libXCTCompatShim.dylib` and the runner, kept the .xctest (it only removes .appex
  extensions for one-ID profiles), and its own installer-style check said clean.
Differences from iCTRL's `sign-ipa.sh`: Signer also puts a copy of the profile and the app's
entitlements on the .xctest; the script signs the .xctest without either. Probably harmless; if the
runner crashes at launch when signed by Signer, change that first. Not yet tried on the phone. If it
works there, steps 1's GitHub job and its three secrets are not needed: sign the runner in Signer.

**7:00 PM Eastern, on the owner's phone:** he installed the runner (v1.98.12) and tapped it; it went
straight back to the Home Screen with the notification "iCTRL Server Ready, This device only, Running on
127.0.0.1:8200 (local only)". That is the runner working: it has no screen, only that notification.
(A copy signed by ESign or Feather would close at launch, per iCTRL's README.) Left for later: the iSH
tunnel and pairing; "This device only" should be enough for it, because the tunnel starts on the phone.
