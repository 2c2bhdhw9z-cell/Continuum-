#!/bin/sh
# Kiro Home stand-in for the native launcher. kiro-home-native.sh bind-mounts this file (in RAM
# only) over LG's Home program, /usr/bin/com.webos.app.home, the same way kiro-home.sh does for the
# web launcher. SAM runs it whenever Home is launched; it becomes the native launcher
# (native/kiro-home --home, a copy of Continuum's continuum-tv), which registers with SAM itself
# and comes to the front on the Home key. SAM is never restarted.
K=/media/developer/kiro
H=$K/home
. $K/lib.sh
LOG=/tmp/kiro-home.log
log() { echo "$(up) native-standin[$$]: $*" >> $LOG 2>/dev/null; }

# one Home at a time: an earlier copy (and its power watch) goes
pkill -f "$H/native/kiro-home --home" 2>/dev/null
pkill -f "[l]una-send -i luna://com.webos.service.tvpower/power/getPowerState" 2>/dev/null

# LG's Home opens itself when the TV wakes from standby and Home Auto Launch is on. Do the same,
# unless an app (Netflix key, AirPlay, a cast) is what the TV woke up with.
home_at_power_on() {
  lsend -n 1 -w 3000 luna://com.webos.settingsservice/getSystemSettings '{"category":"general","keys":["homeAutoLaunch"]}' 2>/dev/null |
    grep -q '"homeAutoLaunch": *"on"' || return 0
  fg=''
  for i in 1 2 3 4 5 6; do sleep 1; fg=$(fgapp); [ -n "$fg" ] && break; done
  case "$fg" in
    com.webos.app.home) return 0 ;;
    com.webos.app.hdmi* | com.webos.app.livetv | '') ;;
    *) log "woke up with $fg in front; Home is not opened over it"; return 0 ;;
  esac
  log "woke up (${fg:-nothing in front}); opening Home"
  lsend -n 1 -w 5000 luna://com.webos.applicationManager/launch '{"id":"com.webos.app.home"}' >/dev/null 2>&1
}
power_watch() {
  last=''
  lsend -i luna://com.webos.service.tvpower/power/getPowerState '{"subscribe":true}' 2>/dev/null | while read -r line; do
    st=$(echo "$line" | sed -n 's/.*"state": *"\([^"]*\)".*/\1/p')
    [ -n "$st" ] || continue
    case "$last" in 'Active Standby' | Suspend) [ "$st" = Active ] && home_at_power_on ;; esac
    last=$st
  done
}

log "started as uid $(id -u): $(echo "$*" | cut -c1-160)"
power_watch &
touch /tmp/kiro-home-asked
exec "$H/native/kiro-home" --home "$1"
