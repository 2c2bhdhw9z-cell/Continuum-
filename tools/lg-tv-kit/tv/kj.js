#!/usr/bin/env node
// Small JSON helper for the Kiro TV scripts. Runs only when a script calls it (never stays in memory).
// Every write is validated before it replaces a file, and files are written in place so owner and mode stay as they were.
'use strict';
const fs = require('fs');
const path = require('path');
const [cmd, ...a] = process.argv.slice(2);

function read(f, d) {
  try {
    return JSON.parse(fs.readFileSync(f, 'utf8'));
  } catch (e) {
    if (d !== undefined) return d;
    throw e;
  }
}
function write(f, o) {
  const s = JSON.stringify(o);
  JSON.parse(s);
  fs.writeFileSync(f, s);
}

switch (cmd) {
  // settings <getSystemSettings reply file> -> the "settings" object, or exit 1
  case 'settings': {
    const r = read(a[0], {});
    if (!r.returnValue || !r.settings) process.exit(1);
    console.log(JSON.stringify(r.settings));
    break;
  }
  // only-known <current settings json> <wanted settings json> -> the wanted keys this TV actually has
  case 'only-known': {
    const cur = JSON.parse(a[0]);
    const want = JSON.parse(a[1]);
    const out = {};
    Object.keys(want).forEach(k => {
      if (Object.prototype.hasOwnProperty.call(cur, k)) out[k] = want[k];
    });
    console.log(JSON.stringify(out));
    break;
  }
  // preload <file> <true|false> <id...> -> set allowedApps[id] for ids that are listed
  case 'preload': {
    const o = read(a[0]);
    const v = a[1] === 'true';
    a.slice(2).forEach(id => {
      if (o.allowedApps && Object.prototype.hasOwnProperty.call(o.allowedApps, id)) {
        o.allowedApps[id] = v;
        console.log('  ' + id + ': preload ' + v);
      } else console.log('  ' + id + ': not in the preload list, skipped');
    });
    write(a[0], o);
    break;
  }
  // deleted-add <file> <id...> -> create or extend {"deletedList":[...]}
  case 'deleted-add': {
    const o = read(a[0], { deletedList: [] });
    if (!Array.isArray(o.deletedList)) o.deletedList = [];
    a.slice(1).forEach(id => {
      if (o.deletedList.indexOf(id) < 0) {
        o.deletedList.push(id);
        console.log('  ' + id + ': hidden');
      } else console.log('  ' + id + ': already hidden');
    });
    write(a[0], o);
    break;
  }
  // deleted-remove <file> <id...> -> take ids out of the list again (other entries, e.g. uninstalled apps, stay)
  case 'deleted-remove': {
    const o = read(a[0], null);
    if (!o || !Array.isArray(o.deletedList)) break;
    const drop = a.slice(1);
    o.deletedList = o.deletedList.filter(id => drop.indexOf(id) < 0);
    write(a[0], o);
    console.log('  shown again: ' + drop.join(' '));
    break;
  }
  // visible <appinfo.json> <true|false>
  case 'visible': {
    const o = read(a[0]);
    o.visible = a[1] === 'true';
    write(a[0], o);
    break;
  }
  // installed <listApps reply file> <id> -> exit 0 when installed
  case 'installed': {
    const r = read(a[0], {});
    process.exit((r.apps || []).some(x => x.id === a[1]) ? 0 : 1);
  }
  // stage-home <stock Home appinfo.json> <out appinfo.json>: LG's own Home manifest turned into a web app manifest
  case 'stage-home': {
    const h = read(a[0]);
    ['nativeLifeCycleInterfaceVersion', 'supportQuickStart', 'splashBackground', 'noSplashOnLaunch', 'containerBasedApp', 'windowGroup'].forEach(
      k => delete h[k]
    );
    h.type = 'web';
    h.main = 'index.html';
    h.icon = 'icon.png';
    h.mediumIcon = 'icon.png';
    h.largeIcon = 'icon.png';
    h.bgColor = '#000000';
    h.transparent = false;
    h.title = 'Home';
    fs.writeFileSync(a[1], JSON.stringify(h, null, 1));
    break;
  }
  // wamlaunch <appinfo.json> <folder> <out>: the request the stand-in sends to webapp-mgr (plus <out>.str with appDesc as a string)
  case 'wamlaunch': {
    const w = read(a[0]);
    w.folderPath = a[1];
    const req = { appDesc: w, parameters: {}, launchingAppId: '', launchingProcId: '', instanceId: 'kiro-home' };
    fs.writeFileSync(a[2], JSON.stringify(req));
    req.appDesc = JSON.stringify(w);
    fs.writeFileSync(a[2] + '.str', JSON.stringify(req));
    break;
  }
  // catalog <launch points reply> <input status reply> <app dir>: app/catalog.js and app/icons/ for the launcher
  case 'catalog': {
    const lp = read(a[0], {}).launchPoints || [];
    const inp = read(a[1], {}).devices || [];
    const dir = a[2];
    const icons = path.join(dir, 'icons');
    fs.mkdirSync(icons, { recursive: true });
    const copyIcon = (id, src) => {
      if (!src) return '';
      try {
        const name = id.replace(/[^A-Za-z0-9._-]/g, '_') + path.extname(src);
        fs.copyFileSync(src, path.join(icons, name));
        return 'icons/' + name;
      } catch (e) {
        return '';
      }
    };
    const apps = lp.map(x => ({
      id: x.id,
      title: x.title,
      color: x.iconColor || x.bgColor || '',
      icon: copyIcon(x.id, x.largeIcon || x.mediumLargeIcon || x.icon),
      systemApp: !!x.systemApp,
    }));
    const inputs = inp.map(d => ({
      id: d.appId,
      label: d.label,
      port: d.port,
      name: ((d.subList || [])[0] || {}).osdName || '',
    }));
    const extra = {
      airplay: copyIcon('airplay', '/usr/palm/applications/airplay/AirPlay_Icon-115x115.png'),
    };
    fs.writeFileSync(
      path.join(dir, 'catalog.js'),
      '// written by kiro-home.sh catalog; the launcher refreshes this list live when it can\nwindow.KIRO_CATALOG = ' +
        JSON.stringify({ built: new Date().toISOString(), apps, inputs, extra }, null, 1) +
        ';\n'
    );
    console.log('  catalog: ' + apps.length + ' apps, ' + inputs.length + ' inputs');
    break;
  }
  default:
    console.error('usage: kj.js settings|only-known|preload|deleted-add|deleted-remove|visible|installed|stage-home|wamlaunch|catalog');
    process.exit(2);
}
