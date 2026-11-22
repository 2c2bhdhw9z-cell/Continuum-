#!/bin/sh
# One HTTP request from the Kiro launcher page. BusyBox tcpsvd starts this per connection, listening on 127.0.0.1 only,
# with the socket on stdin/stdout. A page opened this way cannot use the Luna bus itself, so it asks here.
#   GET /launch?id=<appId>   open an app or input
#   GET /apps                the launch points (installed apps)
#   GET /inputs              HDMI inputs and their device names
#   GET /lghome              LG's Home until the next reboot
#   GET /bg                  backgrounds: settings and pictures
#   GET /bg/set?key=&value=  change a background setting (kiro-bg.sh validates it)
#   GET /bg/delete?name=     delete a picture
#   GET /bg/upload           start the phone upload page; returns its address
. /media/developer/kiro/lib.sh
H=/media/developer/kiro/home
read -r method target proto
while read -r line; do
  line=$(printf '%s' "$line" | tr -d '\r')
  [ -z "$line" ] && break
done
path=${target%%\?*}
# one query value; only letters, digits, dot, underscore and dash get through
q() { printf '%s' "$target" | sed -n "s/^[^?]*?\(.*&\)\{0,1\}$1=\([A-Za-z0-9._-]\{1,100\}\).*$/\2/p"; }
id=$(q id)

hdr() {
  printf 'HTTP/1.0 %s\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n' "$1"
}

case "$method $path" in
  'GET /launch')
    if [ -z "$id" ]; then hdr '400 Bad Request'; echo '{"returnValue":false,"errorText":"bad id"}'; exit 0; fi
    hdr '200 OK'
    lsend -n 1 -w 10000 luna://com.webos.applicationManager/launch "{\"id\":\"$id\"}" 2>&1
    ;;
  'GET /apps')
    lsend -n 1 -w 10000 luna://com.webos.applicationManager/listLaunchPoints '{}' > /tmp/kiro-api-apps.json 2>&1
    hdr '200 OK'; cat /tmp/kiro-api-apps.json
    ;;
  'GET /inputs')
    lsend -n 1 -w 8000 luna://com.webos.service.eim/getAllInputStatus '{}' > /tmp/kiro-api-inputs.json 2>&1
    hdr '200 OK'; cat /tmp/kiro-api-inputs.json
    ;;
  'GET /airplay')
    # what LG's Home Dashboard calls for its AirPlay tile: the AirPlay & HomeKit page
    hdr '200 OK'
    lsend -n 1 -w 10000 luna://airplay.service/showAirplayPage '{}' 2>&1
    ;;
  'GET /bg')
    hdr '200 OK'; sh $H/kiro-bg.sh json
    ;;
  'GET /bg/set')
    hdr '200 OK'; sh $H/kiro-bg.sh set "$(q key)" "$(q value)"
    ;;
  'GET /bg/delete')
    hdr '200 OK'; sh $H/kiro-bg.sh delete "$(q name)"
    ;;
  'GET /bg/upload')
    u=$(sh $H/kiro-bg.sh upload)
    hdr '200 OK'
    if [ -n "$u" ]; then printf '{"returnValue":true,"url":"%s"}\n' "$u"
    else echo '{"returnValue":false,"errorText":"the upload page did not start"}'; fi
    ;;
  'GET /lghome')
    hdr '200 OK'; echo '{"returnValue":true}'
    setsid sh /media/developer/kiro/home/kiro-home.sh off </dev/null >/dev/null 2>&1 &
    ;;
  'OPTIONS '*)
    hdr '204 No Content'
    ;;
  *)
    hdr '404 Not Found'; echo '{"returnValue":false,"errorText":"unknown request"}'
    ;;
esac
