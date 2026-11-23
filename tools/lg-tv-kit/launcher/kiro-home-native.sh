#!/bin/sh
# kiro-home-native.sh: the native Kiro launcher (tv/webos/src/home.rs) as this TV's Home screen.
# Runs on the TV as root. RAM only, like kiro-home.sh: nothing on the system partition is written,
# and the boot hook (50-kiro-home, the web launcher) is left alone until the native one has passed
# a cold reboot and the owner has said yes.
#
#   sh kiro-home-native.sh install    copy native/kiro-home and its SDL from Continuum's app folder
#   sh kiro-home-native.sh on         the native launcher is Home now, until the next reboot; if it
#                                     isn't up and in front within 30 seconds, the web one comes back
#   sh kiro-home-native.sh off        LG's own Home until the next reboot (the LG Home button)
#   sh kiro-home-native.sh web        back to the web launcher now
#   sh kiro-home-native.sh status
K=/media/developer/kiro
H=$K/home
N=$H/native
. $K/lib.sh
EXE=/usr/bin/com.webos.app.home
APP=/media/developer/apps/usr/palm/applications/dev.continuum.tv
LOG=/tmp/kiro-home.log
log() { echo "$(up) kiro-home-native: $*" | tee -a $LOG; }
mounted_by() { grep " $EXE " /proc/mounts >/dev/null 2>&1; }

install() {
  mkdir -p $N/lib
  cp $APP/continuum-tv $N/kiro-home.new && mv $N/kiro-home.new $N/kiro-home
  cp $APP/lib/libSDL2-2.0.so.0 $N/lib/libSDL2-2.0.so.0
  chmod 755 $N/kiro-home $H/home-standin-native.sh $H/kiro-home-native.sh
  log "installed in $N"
}

# whatever is Home right now goes: the web launcher (webapp-mgr), its stand-in, the native one
stop_all() {
  pkill -f "^/bin/sh $EXE" 2>/dev/null
  pkill -f "^/bin/sh $H/home-standin.sh" 2>/dev/null
  pkill -f "^/bin/sh $H/home-standin-native.sh" 2>/dev/null
  pkill -f "$N/kiro-home --home" 2>/dev/null
  pkill -f "[l]una-send -i -a com.webos.app.home" 2>/dev/null
  pkill -f "[l]una-send -i luna://com.webos.service.tvpower/power/getPowerState" 2>/dev/null
  lsend -n 1 -w 5000 luna://com.palm.webappmanager/killApp '{"appId":"com.webos.app.home"}' >/dev/null 2>&1
  for p in $(pgrep -f "^$EXE"); do kill "$p" 2>/dev/null; done
  sleep 1
  while mounted_by; do umount $EXE 2>/dev/null || umount -l $EXE || break; done
}

native_up() { pgrep -f "$N/kiro-home --home" >/dev/null; }

on() {
  [ -x $N/kiro-home ] && [ -f $N/lib/libSDL2-2.0.so.0 ] || { log "not installed (run: sh kiro-home-native.sh install)"; return 1; }
  stop_all
  chmod 755 $H/home-standin-native.sh
  mount --bind $H/home-standin-native.sh $EXE || { log "mount over $EXE failed; web launcher back"; sh $H/kiro-home.sh on; return 1; }
  log "native stand-in mounted over $EXE"
  sleep 1
  lsend -n 1 -w 10000 luna://com.webos.applicationManager/launch '{"id":"com.webos.app.home"}' >/dev/null 2>&1
  for i in $(seq 1 15); do
    if native_up && [ "$(fgapp)" = com.webos.app.home ] && grep -q "window " /tmp/kiro-home-native.log 2>/dev/null; then
      log "the native launcher is up and in front"
      return 0
    fi
    sleep 2
  done
  log "the native launcher did not come up; the web launcher is back"
  tail -n 20 /tmp/kiro-home-native.log 2>/dev/null >> $LOG
  web
  return 1
}

web() {
  stop_all
  sh $H/kiro-home.sh on
}

off() {
  stop_all
  sleep 1
  lsend -n 1 -w 10000 luna://com.webos.applicationManager/launch '{"id":"com.webos.app.home"}' >/dev/null 2>&1
  log "LG's Home is back until the next reboot"
}

status() {
  grep -q " $EXE " /proc/mounts && echo "stand-in mounted over $EXE" || echo "nothing over $EXE (LG's Home)"
  native_up && echo "native launcher: running ($(pgrep -f "$N/kiro-home --home" | head -1))" || echo "native launcher: not running"
  echo "front: $(fgapp)"
  for p in $(pgrep -f "$N/kiro-home --home"); do grep -E "VmRSS" /proc/$p/status; done
  tail -n 10 /tmp/kiro-home-native.log 2>/dev/null
}

[ "$(id -u)" = 0 ] || { echo "run as root"; exit 1; }
case "$1" in
  install) install ;;
  on) on ;;
  off) off ;;
  web) web ;;
  status) status ;;
  *) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
