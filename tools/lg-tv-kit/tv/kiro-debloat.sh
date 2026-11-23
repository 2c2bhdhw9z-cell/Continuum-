#!/bin/sh
# kiro-debloat.sh: reversible debloat for this LG 55UQ7570PUJ (webOS 22 / 7.6.0). Runs on the TV as root.
#
#   sh kiro-debloat.sh status    show what is on or off now (changes nothing)
#   sh kiro-debloat.sh apply     ads/promos/recommendations off, preload off, hide tiles, uninstall LG junk, arm boot hook
#   sh kiro-debloat.sh restore   put back everything from the backups (reboot afterwards)
#   sh kiro-debloat.sh boot      run by the boot hook: stop LG's nag/telemetry services and block ad servers (RAM only)
#
# Every file and setting is backed up once to /media/developer/kiro/backup before it is first changed.
# Nothing on the read-only system partition is written.
K=/media/developer/kiro
B=$K/backup
. $K/lib.sh
PRELOAD=/var/preferences/webos-preload-manager-conf.json
DELETED=/var/preferences/com.webos.applicationManager/deletedSystemAppList.json
UPDATED=/media/system/apps/usr/palm/applications
LGCH=$UPDATED/com.webos.app.lgchannels/appinfo.json
HOOK=/var/lib/webosbrew/init.d/40-kiro-debloat

# category|settings to set (only keys this TV reports are written)
SETTINGS='general|{"homePromotion":"off","sportsAlarm":"off","screenSaverAd":"off","adCookie":"off"}
option|{"webOSPromotionVideo":"off","livePlus":"off","livePromotion":"off","promotionStreamer":"off","channelplus":"off","dbgLogUpload":false}
other|{"contentRecommendation":"off"}'

# not kept in memory after boot (they still open, a little slower the first time)
PRELOAD_OFF="com.webos.app.lgchannels com.webos.app.browser com.webos.app.voice"
# hidden from Home and the app list through webOS's own list of removed system apps
HIDE="com.webos.app.lgchannels com.webos.app.sportsteamsettings amazon.alexa.view com.webos.app.roomconnect com.webos.app.store-demo com.webos.app.sync-demo com.webos.app.browser com.webos.app.mediadiscovery"
# LG-branded store apps that came preinstalled; all can be reinstalled from the LG Content Store
UNINSTALL="com.lgshop.app com.lgtvod.us.app com.twin.app.audiochannel com.lgtravel.app com.volley.lgshell com.miniclip.eightballpool com.twin.app.gamingportal epoxyaisports com.twin.app.homegym de.2kit.castbrowserlg
 com.movenetworks.app.sling-tv-sling-production youtube.leanback.ytv.v1 com.fubotv.app com.peacock.tv com.apple.appletv com.amazon.luna
 amazon com.wbd.stream com.cbs-all-access.webapp.prod hulu com.disney.disneyplus-prod"
# never hidden, uninstalled or taken out of memory
PROTECTED=" com.webos.app.livetv com.webos.app.home com.webos.app.discovery com.palm.app.settings com.webos.app.inputcommon
 com.webos.app.hdmi1 com.webos.app.hdmi2 com.webos.app.hdmi3 com.webos.app.hdmi4 com.webos.app.homeconnect airplay
 com.webos.app.gameoptimizer com.webos.app.livemenu com.webos.app.softwareupdate com.webos.app.notificationcenter
 org.webosbrew.hbchannel org.webosbrew.inputhook netflix youtube.leanback.v4 com.disney.disneyplus-prod hulu "
# LG services that only serve recommendations, nudges, LG Channels, telemetry logs or Matter smart-home (stopped until the next boot)
# iot-client is LG ThinQ (phone-app control); the iPhone's Control Center remote goes through AirPlay instead
SERVICES="contentminer nudge lgchannelurl remotelogger service-logger chip-service iot-client"
# LG ad, beacon, recommendation and ACR servers (US). App store, account and streaming hosts are deliberately not listed.
ADHOSTS="ad.lgsmartad.com info.lgsmartad.com us.lgsmartad.com us.ad.lgsmartad.com us.info.lgsmartad.com
 ads.lgtvcommon.com aic.ads.lgtvcommon.com eic.ads.lgtvcommon.com cdpbeacon.lgtvcommon.com cdpbeacon2.lgtvcommon.com
 aic.cdpbeacon.lgtvcommon.com aic.cdpbeacon2.lgtvcommon.com aic.recommend.lgtvcommon.com aic.nudge.lgtvcommon.com
 nudge.lgtvcommon.com us.rdx2.lgtvsdp.com alphonso.tv tkacrs.alphonso.tv lgads.tv"

say() { printf '%s\n' "$*"; }
protected() { case "$PROTECTED" in *" $1 "*) return 0 ;; esac; return 1; }
backup_once() { # $1 file, $2 name
  [ -e "$B/$2" ] || [ -e "$B/$2.absent" ] && return 0
  mkdir -p "$B"
  if [ -e "$1" ]; then cp -p "$1" "$B/$2" && say "  backed up $1"; else touch "$B/$2.absent"; fi
}

settings_off() {
  say "== Ads, promotions, recommendations, ACR"
  echo "$SETTINGS" | while IFS='|' read -r cat want; do
    keys=$(node -e 'console.log(JSON.stringify(Object.keys(JSON.parse(process.argv[1]))))' "$want")
    lsend -n 1 -w 8000 luna://com.webos.settingsservice/getSystemSettings "{\"category\":\"$cat\",\"keys\":$keys}" > /tmp/kiro-s.json
    cur=$(node $K/kj.js settings /tmp/kiro-s.json) || { say "  $cat: could not read, skipped"; continue; }
    mkdir -p "$B"
    [ -s "$B/settings-$cat.json" ] || printf '%s\n' "$cur" > "$B/settings-$cat.json"
    new=$(node $K/kj.js only-known "$cur" "$want")
    r=$(lsend -n 1 -w 8000 luna://com.webos.settingsservice/setSystemSettings "{\"category\":\"$cat\",\"settings\":$new}")
    case "$r" in *'"returnValue":true'* | *'"returnValue": true'*) say "  $cat: was $cur, now $new" ;; *) say "  $cat: FAILED $r" ;; esac
  done
}
settings_restore() {
  say "== Settings back to their saved values"
  for cat in general option other; do
    f="$B/settings-$cat.json"
    [ -s "$f" ] || continue
    r=$(lsend -n 1 -w 8000 luna://com.webos.settingsservice/setSystemSettings "{\"category\":\"$cat\",\"settings\":$(cat "$f")}")
    case "$r" in *'"returnValue":true'* | *'"returnValue": true'*) say "  $cat: $(cat "$f")" ;; *) say "  $cat: FAILED $r" ;; esac
  done
}

preload_off() {
  say "== Preload off (next boot)"
  backup_once "$PRELOAD" webos-preload-manager-conf.json
  ids=""
  for id in $PRELOAD_OFF; do protected "$id" && say "  $id: protected, skipped" || ids="$ids $id"; done
  node $K/kj.js preload "$PRELOAD" false $ids
}

hide_apps() {
  say "== Hide system tiles (next boot)"
  backup_once "$DELETED" deletedSystemAppList.json
  ids=""
  for id in $HIDE; do protected "$id" && say "  $id: protected, skipped" || ids="$ids $id"; done
  node $K/kj.js deleted-add "$DELETED" $ids
  # apps LG updated (LG Channels, the browser) run from a copy in /media/system/apps, which the deleted list does not reach
  for id in $ids; do
    a=$UPDATED/$id/appinfo.json
    [ -f "$a" ] || continue
    backup_once "$a" "appinfo-$id.json"
    node $K/kj.js visible "$a" false && say "  $id: visible:false in its updated copy"
  done
}

uninstall_junk() {
  say "== Uninstall LG junk apps"
  lsend -n 1 -w 10000 luna://com.webos.applicationManager/listApps '{}' > /tmp/kiro-apps.json
  for id in $UNINSTALL; do
    protected "$id" && { say "  $id: protected, skipped"; continue; }
    node $K/kj.js installed /tmp/kiro-apps.json "$id" || { say "  $id: not installed"; continue; }
    r=$(lsend -n 1 -w 30000 luna://com.webos.appInstallService/remove "{\"id\":\"$id\",\"subscribe\":false}")
    case "$r" in
      *'"returnValue":true'* | *'"returnValue": true'*) say "  $id: removed"; echo "$id" >> "$B/uninstalled.txt" ;;
      *) say "  $id: FAILED $r" ;;
    esac
  done
}

boot() {
  sleep 30
  hosts_block
  # --no-block: "nudge" can take its full stop timeout, and one slow service must not hold up the others
  say "$(up) boot: stopping $SERVICES"
  for s in $SERVICES; do systemctl stop --no-block "$s.service" 2>/dev/null; done
}

hosts_block() {
  # /etc/hosts is on the read-only system partition; the Homebrew Channel already mounts a RAM copy over it when it blocks
  # updates. Add our lines to that copy, or make our own RAM copy if it is not there.
  if ! grep -q ' /etc/hosts ' /proc/mounts; then
    cp /etc/hosts /tmp/kiro-hosts && mount --bind /tmp/kiro-hosts /etc/hosts
  fi
  if grep -q ' /etc/hosts ' /proc/mounts && ! grep -q 'kiro-debloat' /etc/hosts; then
    { echo ''; echo '# kiro-debloat: LG ad/ACR servers (RAM only, rebuilt at boot)'
      for h in $ADHOSTS; do echo "127.0.0.1 $h"; echo "::1 $h"; done; } >> /etc/hosts
    say "$(up) boot: ad servers blocked in /etc/hosts"
  fi
}

status() {
  say "== Settings"
  for cat in general option other; do
    want=$(echo "$SETTINGS" | sed -n "s/^$cat|//p")
    keys=$(node -e 'console.log(JSON.stringify(Object.keys(JSON.parse(process.argv[1]))))' "$want")
    lsend -n 1 -w 8000 luna://com.webos.settingsservice/getSystemSettings "{\"category\":\"$cat\",\"keys\":$keys}" > /tmp/kiro-s.json
    say "  $cat: $(node $K/kj.js settings /tmp/kiro-s.json 2>/dev/null)"
  done
  say "== Preload file"; say "  $(cat $PRELOAD)"
  say "== Hidden system apps"; say "  $(cat $DELETED 2>/dev/null || echo none)"
  say "== Services"; for s in $SERVICES; do say "  $s: $(systemctl is-active $s.service 2>/dev/null)"; done
  say "== Ad hosts"; say "  $(grep -c 'lgsmartad\|lgtvcommon\|alphonso' /etc/hosts) lines in /etc/hosts"
  say "== Boot hook"; [ -e $HOOK ] && say "  $HOOK present" || say "  not armed"
  say "== Backups"; ls $B 2>/dev/null | sed 's/^/  /'
}

restore() {
  settings_restore
  say "== Files"
  # the deleted list also records the apps that were uninstalled, so only our hidden ids come out of it
  [ -f "$DELETED" ] && node $K/kj.js deleted-remove "$DELETED" $HIDE
  for b in "$B"/appinfo-*.json; do
    [ -e "$b" ] || continue
    id=$(basename "$b" .json); id=${id#appinfo-}
    cat "$b" > "$UPDATED/$id/appinfo.json" && say "  $id: appinfo.json restored"
  done
  for pair in "webos-preload-manager-conf.json|$PRELOAD" "lgchannels-appinfo.json|$LGCH"; do
    n=${pair%%|*}; f=${pair#*|}
    if [ -e "$B/$n" ]; then cat "$B/$n" > "$f" && say "  $f restored"
    elif [ -e "$B/$n.absent" ]; then rm -f "$f" && say "  $f removed (it did not exist before)"; fi
  done
  rm -f $HOOK && say "  boot hook removed"
  [ -s "$B/uninstalled.txt" ] && say "  reinstall from the LG Content Store if wanted: $(tr '\n' ' ' < $B/uninstalled.txt)"
  say "Reboot to finish."
}

arm() {
  cp $K/40-kiro-debloat $HOOK && chmod 755 $HOOK && say "== Boot hook armed: $HOOK"
}

[ "$(id -u)" = 0 ] || { say "run as root"; exit 1; }
case "$1" in
  status) status ;;
  # uninstall first: SAM rewrites deletedSystemAppList.json from memory after each removal, so hide afterwards
  apply) settings_off; preload_off; uninstall_junk; sleep 20; hide_apps; arm; say "Reboot to finish (preload and hidden tiles apply at boot)." ;;
  hide) hide_apps ;;
  restore) restore ;;
  boot) boot ;;
  hosts) hosts_block ;;
  *) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
