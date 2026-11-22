#!/bin/sh
# kiro-bg.sh: pictures for the Kiro launcher's background and the screen saver. Runs on the TV as root.
#   sh kiro-bg.sh json | refresh             current settings and pictures
#   sh kiro-bg.sh set mode none|one|shuffle
#   sh kiro-bg.sh set current <picture>      use one picture (shuffle off)
#   sh kiro-bg.sh set interval 1|5|15|30|60  minutes between pictures when shuffling
#   sh kiro-bg.sh set saver on|off           pictures as the screen saver (RAM-only mount over two of LG's screen saver files)
#   sh kiro-bg.sh delete <picture>
#   sh kiro-bg.sh import <file.jpg> [thumb.jpg]
#   sh kiro-bg.sh upload                     start the phone upload page and print its address
#   sh kiro-bg.sh boot                       run at boot by kiro-home.sh
K=/media/developer/kiro
H=$K/home
. $K/lib.sh
BG=$H/app/bg
CONF=$H/bg.conf
SS=/usr/palm/applications/com.webos.app.screensaver/qml/UserInterfaceLayer/Containers
SLIDES=$H/screensaver/Slideshow.qml
LOG=/tmp/kiro-home.log
log() { echo "$(up) kiro-bg: $*" >> $LOG; }
mkdir -p $BG/thumbs

valid() { printf '%s' "$1" | grep -Eq '^p[a-z0-9]{6,20}\.jpg$'; }
get() { sed -n "s/^$1=//p" $CONF 2>/dev/null | tail -n 1; }
put() { { grep -v "^$1=" $CONF 2>/dev/null; echo "$1=$2"; } > $CONF.new && mv $CONF.new $CONF; }
files() { ls -t $BG 2>/dev/null | grep -E '^p[a-z0-9]{6,20}\.jpg$'; }
fail() { printf '{"returnValue":false,"errorText":"%s"}\n' "$1"; exit 0; }

json() {
  mode=$(get mode); cur=$(get current); iv=$(get interval); sv=$(get saver)
  [ -n "$mode" ] || mode=none
  [ -n "$iv" ] || iv=15
  [ -n "$sv" ] || sv=off
  list=$(files | sed 's/.*/"&"/' | tr '\n' ',' | sed 's/,$//')
  printf '{"returnValue":true,"mode":"%s","current":"%s","interval":%s,"saver":"%s","files":[%s]}\n' "$mode" "$cur" "$iv" "$sv" "$list"
}
refresh() { printf 'window.KIRO_BG = %s;\n' "$(json)" > $H/app/bg.js.new && mv $H/app/bg.js.new $H/app/bg.js; }

saver_restart() {
  lsend -n 1 -w 4000 luna://com.webos.applicationManager/closeByAppId '{"id":"com.webos.app.screensaver"}' >/dev/null 2>&1
  pkill -f "[q]ml-runner.*com.webos.app.screensaver" 2>/dev/null
  return 0
}
saver_mounted() { grep -q " $SS/ScreenSaver.qml " /proc/mounts; }
saver_on() {
  [ -n "$(files)" ] || { saver_off; return 0; }
  [ -f $SLIDES ] || { log "missing $SLIDES"; return 1; }
  saver_mounted && return 0
  if mount --bind $SLIDES $SS/ScreenSaver.qml && mount --bind $SLIDES $SS/Clock.qml; then
    log "screen saver: your pictures"
  else
    log "screen saver mount failed; LG's stays"
    umount $SS/ScreenSaver.qml 2>/dev/null
    return 1
  fi
  saver_restart
}
saver_off() {
  m=0
  for f in ScreenSaver.qml Clock.qml; do
    grep -q " $SS/$f " /proc/mounts && umount $SS/$f && m=1
  done
  [ $m = 1 ] && { log "screen saver: LG's"; saver_restart; }
  return 0
}

set_() {
  case "$1=$2" in
    mode=none | mode=shuffle) put mode "$2" ;;
    mode=one) put mode one; c=$(get current); [ -n "$c" ] && [ -f "$BG/$c" ] || put current "$(files | head -n 1)" ;;
    current=*) { valid "$2" && [ -f "$BG/$2" ]; } || fail "no such picture"; put current "$2"; put mode one ;;
    interval=1 | interval=5 | interval=15 | interval=30 | interval=60) put interval "$2" ;;
    saver=on) put saver on; saver_on ;;
    saver=off) put saver off; saver_off ;;
    *) fail "bad setting" ;;
  esac
  refresh
  json
}

# a picture arrived (upload page or import): the very first one becomes the background
added() {
  valid "$1" || return 0
  c=$(get current)
  if [ -z "$c" ] || [ ! -f "$BG/$c" ]; then
    put current "$1"
    [ "$(get mode)" = shuffle ] || put mode one
  fi
  refresh
  [ "$(get saver)" = on ] && saver_on
  return 0
}

delete() {
  valid "$1" || fail "no such picture"
  rm -f "$BG/$1" "$BG/thumbs/$1"
  [ "$(get current)" = "$1" ] && put current "$(files | head -n 1)"
  [ -n "$(files)" ] || { put mode none; put current ""; saver_off; }
  refresh
  json
}

import_() {
  [ -f "$1" ] || { echo "no such file: $1"; exit 1; }
  n=p$(date +%s)$(tr -dc a-z0-9 </dev/urandom | head -c 4).jpg
  cp "$1" "$BG/$n" && cp "${2:-$1}" "$BG/thumbs/$n" && added "$n" && echo "imported as $n"
}

upload() {
  U=/tmp/kiro-upload.url
  if pgrep -f "[n]ode $H/upload.js" >/dev/null && [ -s $U ]; then cat $U; return 0; fi
  pkill -f "[n]ode $H/upload.js" 2>/dev/null
  ip=$(ip -4 route get 1.1.1.1 2>/dev/null | sed -n 's/.* src \([0-9.]*\).*/\1/p')
  [ -n "$ip" ] || return 1
  code=$(tr -dc a-z0-9 </dev/urandom | head -c 5)
  echo "http://$ip:8767/$code" > $U
  setsid node $H/upload.js "$code" </dev/null >>/tmp/kiro-upload.log 2>&1 &
  # node takes a few seconds to start on this chip: answer only once the page is really there
  for i in $(seq 1 20); do
    curl -s -m 2 -o /dev/null "http://127.0.0.1:8767/$code" && { cat $U; return 0; }
    sleep 1
  done
  return 1
}

boot() {
  refresh
  [ "$(get saver)" = on ] && saver_on
  return 0
}

case "$1" in
  json) json ;;
  refresh) refresh ;;
  set) set_ "$2" "$3" ;;
  delete) delete "$2" ;;
  added) added "$2" ;;
  import) import_ "$2" "$3" ;;
  upload) upload ;;
  boot) boot ;;
  saver-on) saver_on ;;
  saver-off) saver_off ;;
  *) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
