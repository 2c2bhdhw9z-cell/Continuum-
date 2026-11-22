#!/bin/sh
# Kiro Home stand-in. kiro-home.sh bind-mounts this file (in RAM only) over LG's Home program, /usr/bin/com.webos.app.home.
# The app manager (SAM) then runs this script whenever Home is launched, and the script asks webapp-mgr to open the Kiro
# launcher page as com.webos.app.home. SAM is never restarted. A reboot without the boot hook brings LG's Home back.
# Approach adapted from CleanYourTV's home-standin.sh (https://github.com/lasharor/cleanyourtv-WebOS, MIT licence).
K=/media/developer/kiro
H=$K/home
. $K/lib.sh
LOG=/tmp/kiro-home.log
log() { echo "$(up) standin[$$]: $*" >> $LOG 2>/dev/null; }

# a copy that SAM killed earlier can leave its registration and power watch behind
for p in $(pgrep -f "^/bin/sh $0"); do [ "$p" != $$ ] && kill "$p" 2>/dev/null; done
pkill -f "[l]una-send -i -a com.webos.app.home" 2>/dev/null
pkill -f "[l]una-send -i luna://com.webos.service.tvpower/power/getPowerState" 2>/dev/null

show() { # $1 start|relaunch, $2 SAM's argument or event
  case "$2" in
    *'"reason":"preload"'* | *'"reason": "preload"'*)
      log "preloaded by SAM: the launcher opens on the Home key"
      return 0 ;;
  esac
  touch /tmp/kiro-home-asked
  for f in $H/wam-launch.json $H/wam-launch.json.str; do
    [ -f $f ] || continue
    r=$(lsend -n 1 -w 8000 luna://com.palm.webappmanager/launchApp "$(cat $f)" 2>&1 | tr -d '\n')
    case "$r" in *'"returnValue":true'* | *'"returnValue": true'*) log "launcher shown ($1)"; return 0 ;; esac
    log "webapp-mgr refused ${f##*/}: $(echo "$r" | cut -c1-200)"
  done
  return 1
}

# When SAM restarts (some TVs do that on standby) it no longer knows this script as Home: close the launcher and end,
# so the next Home launch starts everything afresh.
sam_restarted() {
  log "SAM restarted; closing the launcher so Home starts afresh"
  for i in $(seq 1 30); do
    lsend -n 1 -w 1000 luna://com.webos.applicationManager/running '{}' 2>/dev/null | grep -q '"returnValue": *true' && break
    sleep 1
  done
  lsend -n 1 -w 5000 luna://com.palm.webappmanager/killApp '{"appId":"com.webos.app.home"}' >/dev/null 2>&1
  pkill -f "[l]una-send -i -a com.webos.app.home" 2>/dev/null
  pkill -f "[l]una-send -i luna://com.webos.service.tvpower/power/getPowerState" 2>/dev/null
  kill $$ 2>/dev/null
  exit 0
}

# LG's Home opens itself when the TV wakes from standby and Home Auto Launch is on. Do the same, unless an app
# (Netflix key, AirPlay, a cast) is what the TV woke up with.
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
show start "$1"

# Register with SAM as Home, so the Home key sends a "relaunch" event that brings the launcher back to the front.
for m in registerApp registerNativeApp; do
  lsend -i -a com.webos.app.home "luna://com.webos.applicationManager/$m" '{}' 2>&1 | while read -r line; do
    case "$line" in
      *'"event"'*relaunch*) show relaunch "$line" ;;
      *'"event"'*close*) log "closed by SAM"; kill $$ 2>/dev/null; exit 0 ;;
      *'"event"'*registered*) log "registered with SAM ($m)" ;;
      *'com.webos.applicationManager is not running'*) sam_restarted ;;
      *'"returnValue"'*false*) log "$m refused: $(echo "$line" | cut -c1-200)" ;;
    esac
  done
  log "$m ended"
done
log "running without registration: SAM restarts this script on the Home key"
while :; do sleep 3600; done
