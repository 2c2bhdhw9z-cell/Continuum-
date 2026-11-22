# Shared helpers for the Kiro TV scripts (sourced, BusyBox ash on webOS).
export PATH=/usr/bin:/usr/sbin:/bin:/sbin:$PATH
up() { cut -d' ' -f1 /proc/uptime; }

# luna-send with stdin held open. On webOS 7.6.0 luna-send can drop its reply when stdin reaches EOF
# (reported on a 70UQ7070ZUD, firmware 04.60.90), so each call gets a private FIFO that never ends.
lsend() (
  t=$(mktemp -u /tmp/kiro-luna.XXXXXX) && mkfifo "$t" || exit 1
  exec 3<>"$t"
  rm -f "$t"
  exec luna-send "$@" <&3 3<&-
)

# appId of the app in front, or empty
fgapp() {
  lsend -n 1 -w 4000 luna://com.webos.applicationManager/getForegroundAppInfo '{}' 2>/dev/null |
    sed -n 's/.*"appId": *"\([^"]*\)".*/\1/p'
}
