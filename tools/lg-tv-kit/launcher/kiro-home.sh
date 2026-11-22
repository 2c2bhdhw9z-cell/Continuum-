#!/bin/sh
# kiro-home.sh: the Kiro launcher as this TV's Home screen. Runs on the TV as root.
#
#   sh kiro-home.sh install     stage the launcher (manifest from LG's own Home, icons, app list); changes nothing else
#   sh kiro-home.sh catalog     rebuild the app list and icons the launcher falls back to
#   sh kiro-home.sh on          make the launcher Home now, until the next reboot (RAM only)
#   sh kiro-home.sh off         give Home back to LG now
#   sh kiro-home.sh arm         start it at every boot (Homebrew Channel boot hook)
#   sh kiro-home.sh disarm      stop starting it at boot (LG's Home after the next reboot)
#   (backgrounds and the screen saver: kiro-bg.sh)
#   sh kiro-home.sh status
#
# Nothing on the system partition is written: the stand-in is a bind mount that lives in RAM.
K=/media/developer/kiro
H=$K/home
. $K/lib.sh
EXE=/usr/bin/com.webos.app.home
STOCK=/usr/palm/applications/com.webos.app.home
HOOK=/var/lib/webosbrew/init.d/50-kiro-home
LOG=/tmp/kiro-home.log
log() { echo "$(up) kiro-home: $*" | tee -a $LOG; }
mounted() { grep -q " $EXE " /proc/mounts; }

install() {
  mkdir -p $H/app $K/backup
  [ -f $K/backup/home-appinfo.orig.json ] || cp -p $STOCK/appinfo.json $K/backup/home-appinfo.orig.json
  node $K/kj.js stage-home $K/backup/home-appinfo.orig.json $H/app/appinfo.json
  cp $STOCK/assets/icon.png $H/app/icon.png
  node $K/kj.js wamlaunch $H/app/appinfo.json $H/app $H/wam-launch.json
  chmod 755 $H/home-standin.sh
  catalog
  log "installed in $H"
}

catalog() {
  lsend -n 1 -w 10000 luna://com.webos.applicationManager/listLaunchPoints '{}' > /tmp/kiro-lp.json
  lsend -n 1 -w 8000 luna://com.webos.service.eim/getAllInputStatus '{}' > /tmp/kiro-inputs.json
  node $K/kj.js catalog /tmp/kiro-lp.json /tmp/kiro-inputs.json $H/app
}

stop_standin() {
  pkill -f "^/bin/sh $EXE" 2>/dev/null
  pkill -f "^/bin/sh $H/home-standin.sh" 2>/dev/null
  pkill -f "[l]una-send -i -a com.webos.app.home" 2>/dev/null
  pkill -f "[l]una-send -i luna://com.webos.service.tvpower/power/getPowerState" 2>/dev/null
}

off() {
  stop_standin
  sleep 1
  if mounted; then umount $EXE 2>/dev/null || umount -l $EXE; log "stand-in removed from $EXE"; fi
  lsend -n 1 -w 5000 luna://com.palm.webappmanager/killApp '{"appId":"com.webos.app.home"}' >/dev/null 2>&1
  sleep 2
  lsend -n 1 -w 10000 luna://com.webos.applicationManager/launch '{"id":"com.webos.app.home"}' >/dev/null 2>&1
  log "LG's Home is back until the next reboot"
}

launcher_open() { lsend -n 1 -w 3000 luna://com.palm.webappmanager/listRunningApps '{}' 2>/dev/null | grep -q '"com.webos.app.home"'; }

# the launcher page opens apps through this tiny local helper (BusyBox tcpsvd, 127.0.0.1:8766, one short shell per request)
api() {
  pgrep -f "[t]cpsvd .*127.0.0.1 8766" >/dev/null && return 0
  chmod 755 $H/kiro-api.sh
  setsid tcpsvd -c 4 -E 127.0.0.1 8766 /bin/sh $H/kiro-api.sh </dev/null >/dev/null 2>&1 &
  sleep 1
  pgrep -f "[t]cpsvd .*127.0.0.1 8766" >/dev/null && log "helper listening on 127.0.0.1:8766" || log "helper did not start"
}

on() {
  [ -f $H/wam-launch.json ] && [ -f $H/app/index.html ] || { log "not installed (run: sh kiro-home.sh install)"; return 1; }
  api
  if mounted; then log "already on"; return 0; fi
  chmod 755 $H/home-standin.sh
  mount --bind $H/home-standin.sh $EXE || { log "mount over $EXE failed"; return 1; }
  log "stand-in mounted over $EXE"
  rm -f /tmp/kiro-home-asked
  # what is in front before LG's Home goes away: at boot LG's Home is drawn over the last input, and that input is what
  # shows once LG's Home is stopped, so the decision has to be made from what was in front first
  fg=$(fgapp)
  # LG's Home that runs now is its own program: stop it once; from now on SAM starts the stand-in for Home
  for p in $(pgrep -f "^$EXE"); do kill "$p" 2>/dev/null; done
  sleep 2
  # LG's "Power On Screen" setting (homeAutoLaunch) opens Home over the last input at power-on; keep doing that
  auto=$(lsend -n 1 -w 4000 luna://com.webos.settingsservice/getSystemSettings '{"category":"general","keys":["homeAutoLaunch"]}' 2>/dev/null |
    grep -c '"homeAutoLaunch": *"on"')
  case "$fg" in
    com.webos.app.hdmi* | com.webos.app.livetv | com.webos.app.externalinput.*) [ "$auto" -ge 1 ] && fg='' ;;
  esac
  case "$fg" in
    '' | com.webos.app.home) lsend -n 1 -w 10000 luna://com.webos.applicationManager/launch '{"id":"com.webos.app.home"}' >/dev/null 2>&1 ;;
    *)
      # the TV is on an input or in an app: do not cover it; check the launcher the first time Home is opened
      log "the TV is on $fg; the launcher opens on the Home key"
      until [ -f /tmp/kiro-home-asked ]; do
        mounted || return 0
        [ "$(fgapp)" = com.webos.app.home ] && break
        sleep 3
      done ;;
  esac
  for i in $(seq 1 15); do
    if launcher_open; then
      log "the launcher is open in webapp-mgr"
      rm -f $H/fail-count
      return 0
    fi
    sleep 2
  done
  n=$(( $(cat $H/fail-count 2>/dev/null || echo 0) + 1 ))
  echo $n > $H/fail-count
  log "the launcher did not open (failure $n); LG's Home is back"
  off
  # two failed boots in a row: stay on LG's Home until someone looks at it
  [ $n -ge 2 ] && touch $H/DISABLE && log "disabled after $n failures (rm $H/DISABLE to try again)"
  return 1
}

boot() {
  # pictures and the screen saver choice first, before LG's screen saver is loaded
  sh $H/kiro-bg.sh boot </dev/null >/dev/null 2>&1
  [ -e $H/DISABLE ] && { log "disabled ($H/DISABLE)"; return 0; }
  for i in $(seq 1 30); do
    lsend -n 1 -w 2000 luna://com.palm.webappmanager/listRunningApps '{}' 2>/dev/null | grep -q returnValue && break
    sleep 2
  done
  on
  # refresh the fallback app list later, when the TV is idle
  (sleep 120; catalog >/dev/null 2>&1) &
}



status() {
  mounted && echo "stand-in: mounted over $EXE" || echo "stand-in: not mounted (LG's Home)"
  pgrep -f "[t]cpsvd .*127.0.0.1 8766" >/dev/null && echo "helper: listening on 127.0.0.1:8766" || echo "helper: not running"
  launcher_open && echo "launcher: open in webapp-mgr" || echo "launcher: not open"
  echo "front: $(fgapp)"
  [ -e $HOOK ] && echo "boot hook: $HOOK" || echo "boot hook: not armed"
  [ -e $H/DISABLE ] && echo "DISABLED: $H/DISABLE exists"
  tail -n 15 $LOG 2>/dev/null
}

[ "$(id -u)" = 0 ] || { echo "run as root"; exit 1; }
case "$1" in
  install) install ;;
  catalog) catalog ;;
  on) rm -f $H/DISABLE; on ;;
  api) api ;;
  off) off ;;
  boot) boot ;;
  arm) cp $H/50-kiro-home $HOOK && chmod 755 $HOOK && log "boot hook armed: $HOOK" ;;
  disarm) rm -f $HOOK && log "boot hook removed" ;;
  disable) touch $H/DISABLE; off ;;

  status) status ;;
  *) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
