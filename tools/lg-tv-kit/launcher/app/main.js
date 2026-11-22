/* Kiro launcher: a plain, ad-free Home screen. ES5 on purpose (the TV's web engine is old), no framework, no network. */
(function () {
  'use strict';

  var CAT = window.KIRO_CATALOG || { apps: [], inputs: [], extra: {} };
  var BGS = window.KIRO_BG || { mode: 'none', current: '', interval: 15, saver: 'off', files: [] };
  var COLS = 6;
  // mainstream apps first, in this order; anything else installed follows alphabetically
  var ORDER = ['netflix', 'youtube.leanback.v4', 'amazon', 'com.disney.disneyplus-prod', 'hulu', 'com.wbd.stream',
    'com.peacock.tv', 'com.cbs-all-access.webapp.prod', 'com.apple.appletv', 'youtube.leanback.ytv.v1',
    'com.movenetworks.app.sling-tv-sling-production', 'com.fubotv.app', 'com.tubitv.ott.tubi', 'com.plutotv.app',
    'com.apple.applemusic', 'com.amazon.luna'];
  // never shown as app tiles: they are in the top bar, or LG extras that may come back after an update
  var SKIP = {
    'org.webosbrew.hbchannel': 1, 'org.webosbrew.inputhook': 1, 'com.webos.app.discovery': 1, 'com.lgshop.app': 1, 'com.lgtvod.us.app': 1,
    'com.twin.app.audiochannel': 1, 'com.lgtravel.app': 1, 'com.volley.lgshell': 1, 'com.miniclip.eightballpool': 1,
    'com.twin.app.gamingportal': 1, 'epoxyaisports': 1, 'com.twin.app.homegym': 1, 'de.2kit.castbrowserlg': 1
  };
  var BAR = [
    { title: 'Inputs', id: 'com.webos.app.homeconnect' },
    { title: 'Backgrounds', action: 'bgpanel' },
    { title: 'Upload', action: 'bg-add' },
    { title: 'Settings', id: 'com.palm.app.settings' },
    { title: 'App Store', id: 'com.webos.app.discovery' },
    { title: 'Homebrew', id: 'org.webosbrew.hbchannel' },
    { title: 'LG Home', action: 'lghome' }
  ];
  var INTERVALS = [1, 5, 15, 30, 60];

  var $ = function (id) { return document.getElementById(id); };
  var bar = $('bar'), inputsEl = $('inputs'), appsEl = $('apps'), main = $('main');
  var panel = $('panel'), pbar = $('pbar'), pgrid = $('pgrid'), pbody = $('pbody'), dialog = $('dialog');
  // view: 'home', 'panel' (Backgrounds) or 'dialog' (Add pictures)
  var view = 'home', homeKey = '';
  var rows = [], cur = null, lastKey = '', appsKey = '', inputsKey = '', busyUntil = 0, moved = false, redArmed = null;

  // ---- the local helper (kiro-api.sh on 127.0.0.1:8766) does what a page opened this way may not: talk to the Luna bus ----
  var API = 'http://127.0.0.1:8766';
  function api(path, cb) {
    var x = new XMLHttpRequest();
    x.timeout = 15000;
    x.onload = function () {
      var r;
      try { r = JSON.parse(x.responseText); } catch (e) { r = { returnValue: false, errorText: 'bad reply' }; }
      if (cb) cb(r);
    };
    x.onerror = x.ontimeout = function () { if (cb) cb({ returnValue: false, errorText: 'helper not answering' }); };
    x.open('GET', API + path);
    x.send();
  }

  function say(text) {
    var m = $('msg');
    m.textContent = text;
    m.className = 'show';
    clearTimeout(say.t);
    say.t = setTimeout(function () { m.className = ''; }, 5000);
  }

  function launch(id) {
    api('/launch?id=' + encodeURIComponent(id), function (r) {
      if (!r.returnValue) say('Could not open ' + id + ': ' + (r.errorText || r.errorCode || 'no answer'));
    });
  }

  function activate(e) {
    if (!e || !e.kiro) return;
    var now = Date.now();
    if (now < busyUntil) return; // the Magic Remote's OK can arrive as both a click and an Enter key
    busyUntil = now + 700;
    var it = e.kiro;
    switch (it.action) {
      case 'lghome':
        // LG's Home until the next reboot; the Kiro launcher comes back at boot
        api('/lghome', function (r) { if (!r.returnValue) say('Could not switch to LG Home: ' + (r.errorText || 'no answer')); });
        return;
      case 'airplay':
        api('/airplay', function (r) { if (!r.returnValue) say('Could not open AirPlay: ' + (r.errorText || 'no answer')); });
        return;
      case 'bgpanel': openPanel(); return;
      case 'bg-done': closePanel(); return;
      case 'bg-add': openAdd(); return;
      case 'bg-none': bgset('mode', 'none', true); return;
      case 'bg-pick': bgset('current', it.name, true); return;
      case 'bg-shuffle':
        if (BGS.mode !== 'shuffle' && BGS.files.length < 2) { say('Add at least 2 pictures to shuffle'); return; }
        bgset('mode', BGS.mode === 'shuffle' ? 'one' : 'shuffle', true);
        return;
      case 'bg-interval':
        bgset('interval', INTERVALS[(INTERVALS.indexOf(BGS.interval) + 1) % INTERVALS.length], false);
        return;
      case 'bg-saver':
        if (BGS.saver !== 'on' && !BGS.files.length) { say('Add a picture first'); return; }
        bgset('saver', BGS.saver === 'on' ? 'off' : 'on', false);
        return;
    }
    if (it.id) launch(it.id);
  }

  // ---- building the screen ----
  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text) e.textContent = text;
    return e;
  }
  function cellFor(it) {
    var c = el('div', 'cell' + (it.sel ? ' sel' : '')), t = el('div', 'tile' + (it.pic ? ' pic' : ''));
    t.style.backgroundColor = it.color || '#1c2333';
    if (it.icon) {
      var img = new Image();
      img.onerror = function () { if (img.parentNode) t.removeChild(img); t.appendChild(el('div', 'big', it.big || it.title)); };
      img.src = it.icon;
      t.appendChild(img);
    } else {
      t.appendChild(el('div', 'big', it.big || it.title));
      if (it.small) t.appendChild(el('div', 'small', it.small));
    }
    c.appendChild(t);
    c.appendChild(el('div', 'name', it.title));
    c.kiro = it;
    return c;
  }
  function pill(it) { var p = el('div', 'pill', it.title); p.kiro = it; return p; }
  function fill(box, items, make) {
    while (box.firstChild) box.removeChild(box.firstChild);
    for (var i = 0; i < items.length; i++) box.appendChild(make(items[i]));
  }

  function iconFor(id, livePath) {
    for (var i = 0; i < CAT.apps.length; i++) if (CAT.apps[i].id === id && CAT.apps[i].icon) return CAT.apps[i].icon;
    return livePath ? 'file://' + livePath : '';
  }
  function sortApps(list) {
    return list.filter(function (a) { return !a.systemApp && !SKIP[a.id]; }).sort(function (a, b) {
      var x = ORDER.indexOf(a.id), y = ORDER.indexOf(b.id);
      if (x < 0) x = 999;
      if (y < 0) y = 999;
      return x !== y ? x - y : String(a.title).localeCompare(String(b.title));
    });
  }
  function showApps(list) {
    var key = list.map(function (a) { return a.id + a.title; }).join(',');
    if (key === appsKey) return;
    appsKey = key;
    fill(appsEl, list, cellFor);
    if (view === 'home') relink();
  }
  function inputTile(d) {
    // the name comes from the device (HDMI-CEC) or from the input's label in LG's input settings
    var port = 'HDMI ' + d.port;
    var name = d.name || d.label || port;
    var xbox = /xbox/i.test(name);
    return { id: d.id, title: name === port ? 'Input' : port, big: xbox ? 'Xbox' : name, color: xbox ? '#107c10' : '#2a3245' };
  }
  // HDMI 2 is the soundbar's ARC connection: opening it shows nothing
  var HIDE_INPUTS = { 'com.webos.app.hdmi2': 1 };
  function showInputs(devs) {
    var items = devs.filter(function (d) { return /^com\.webos\.app\.hdmi/.test(d.id || '') && !HIDE_INPUTS[d.id]; }).map(inputTile);
    items.push({ action: 'airplay', title: 'AirPlay', icon: (CAT.extra && CAT.extra.airplay) || '', color: '#1c1c1e' });
    var key = items.map(function (i) { return (i.id || i.action) + i.title; }).join(',');
    if (key === inputsKey) return;
    inputsKey = key;
    fill(inputsEl, items, cellFor);
    if (view === 'home') relink();
  }

  // live data when the TV allows it; the catalog written at install time otherwise
  function refresh() {
    api('/apps', function (r) {
      if (!r.returnValue || !r.launchPoints) return;
      showApps(sortApps(r.launchPoints.map(function (x) {
        return { id: x.id, title: x.title, systemApp: x.systemApp, color: x.iconColor || x.bgColor || '', icon: iconFor(x.id, x.largeIcon || x.icon) };
      })));
    });
    api('/inputs', function (r) {
      if (!r.returnValue || !r.devices) return;
      showInputs(r.devices.map(function (d) {
        return { id: d.appId, label: d.label, port: d.port, name: ((d.subList || [])[0] || {}).osdName || '' };
      }));
    });
  }

  // ---- background pictures (files in app/bg, settings kept by kiro-bg.sh) ----
  var layers = [$('bg1'), $('bg2')], front = 0, shownName = '', shuffleTimer = null, bag = [], stamp = Date.now();
  function showPic(name) {
    if (!name) {
      layers[0].style.opacity = 0;
      layers[1].style.opacity = 0;
      document.body.classList.remove('haspic');
      shownName = '';
      return;
    }
    if (name === shownName) return;
    var url = 'bg/' + name, img = new Image();
    img.onload = function () {
      var back = layers[1 - front], old = layers[front];
      back.style.backgroundImage = 'url("' + url + '")';
      back.style.opacity = 1;
      old.style.opacity = 0;
      front = 1 - front;
      shownName = name;
      document.body.classList.add('haspic');
      // let go of the old picture once it has faded out
      setTimeout(function () { if (old.style.opacity === '0') old.style.backgroundImage = 'none'; }, 1700);
    };
    img.src = url;
  }
  function shuffleNext() {
    var f = BGS.files;
    if (!bag.length) {
      bag = f.slice();
      for (var i = bag.length - 1; i > 0; i--) { var j = Math.floor(Math.random() * (i + 1)), t = bag[i]; bag[i] = bag[j]; bag[j] = t; }
    }
    var n = bag.pop();
    if (n === shownName && bag.length) n = bag.pop();
    showPic(n);
  }
  // changeNow: pick a new picture at once (after a choice in the panel); otherwise keep the one on screen
  function applyBg(changeNow) {
    clearInterval(shuffleTimer);
    shuffleTimer = null;
    var f = BGS.files || [];
    if (BGS.mode === 'none' || !f.length) { showPic(''); return; }
    if (BGS.mode === 'shuffle' && f.length > 1) {
      if (changeNow || f.indexOf(shownName) < 0) shuffleNext();
      if (!document.hidden) shuffleTimer = setInterval(shuffleNext, (BGS.interval || 15) * 60000);
      return;
    }
    showPic(f.indexOf(BGS.current) >= 0 ? BGS.current : f[0]);
  }
  function setBgs(r, changeNow) {
    if (!r || !r.files) return false;
    BGS = r;
    bag = [];
    stamp = Date.now();
    applyBg(changeNow);
    return true;
  }
  function bgset(key, value, changeNow) {
    api('/bg/set?key=' + key + '&value=' + encodeURIComponent(value), function (r) {
      if (!r.returnValue) { say('Could not save that: ' + (r.errorText || 'no answer')); return; }
      setBgs(r, changeNow);
      renderPanel();
    });
  }
  function every(n) { return n < 60 ? n + ' min' : '1 hour'; }

  function renderPanel() {
    if (view === 'home') return;
    fill(pbar, [
      { action: 'bg-shuffle', title: 'Shuffle: ' + (BGS.mode === 'shuffle' ? 'On' : 'Off') },
      { action: 'bg-interval', title: 'Change every ' + every(BGS.interval) },
      { action: 'bg-saver', title: 'Screensaver: ' + (BGS.saver === 'on' ? 'On' : 'Off') },
      { action: 'bg-add', title: 'Add pictures' },
      { action: 'bg-done', title: 'Done' }
    ], pill);
    var items = [{ action: 'bg-none', title: BGS.mode === 'none' ? 'In use' : 'No picture', big: 'None', sel: BGS.mode === 'none' }];
    BGS.files.forEach(function (n, i) {
      var inUse = BGS.mode === 'one' && BGS.current === n;
      items.push({ action: 'bg-pick', name: n, pic: true, icon: 'bg/thumbs/' + n + '?v=' + stamp, big: 'Picture ' + (i + 1),
        title: inUse ? 'In use' : (BGS.mode === 'shuffle' ? 'Shuffling' : 'Picture ' + (i + 1)), sel: inUse });
    });
    fill(pgrid, items, cellFor);
    relink();
  }
  function openPanel() {
    homeKey = lastKey;
    view = 'panel';
    panel.className = '';
    lastKey = 'bg-pick:' + BGS.current;
    pbody.scrollTop = 0;
    renderPanel();
  }
  function closePanel() {
    view = 'home';
    panel.className = 'hidden';
    lastKey = homeKey;
    moved = true;
    relink();
  }

  var pollTimer = null, dialogFrom = 'panel';
  function openAdd() {
    api('/bg/upload', function (r) {
      if (!r.returnValue || !r.url) { say('Could not start the upload page: ' + (r.errorText || 'no answer')); return; }
      var before = BGS.files.length;
      dialogFrom = view;
      $('durl').textContent = r.url;
      $('dstat').textContent = 'Waiting for pictures...';
      view = 'dialog';
      dialog.className = '';
      clearInterval(pollTimer);
      pollTimer = setInterval(function () {
        api('/bg', function (b) {
          if (!b.files || view !== 'dialog') return;
          var added = b.files.length - before;
          if (added > 0) $('dstat').textContent = added + (added === 1 ? ' new picture' : ' new pictures') + ' received';
          if (b.files.length !== BGS.files.length) setBgs(b, false);
        });
      }, 4000);
    });
  }
  function closeAdd() {
    clearInterval(pollTimer);
    dialog.className = 'hidden';
    view = dialogFrom;
    api('/bg', function (b) {
      setBgs(b, false);
      if (view === 'panel') renderPanel();
      else { moved = true; relink(); }
    });
  }
  function redKey() {
    if (view !== 'panel' || !cur || !cur.kiro || cur.kiro.action !== 'bg-pick') return;
    if (redArmed !== cur) {
      redArmed = cur;
      say('Press the red button again to delete this picture');
      return;
    }
    redArmed = null;
    api('/bg/delete?name=' + encodeURIComponent(cur.kiro.name), function (r) {
      if (!r.returnValue) { say('Could not delete it: ' + (r.errorText || 'no answer')); return; }
      say('Picture deleted');
      setBgs(r, true);
      renderPanel();
    });
  }

  // ---- remote control ----
  function toArr(n) { return Array.prototype.slice.call(n); }
  function keyOf(e) { return e && e.kiro ? (e.kiro.name ? 'bg-pick:' + e.kiro.name : (e.kiro.id || e.kiro.action)) : ''; }
  function relink() {
    var grid;
    if (view === 'home') {
      rows = [toArr(bar.children), toArr(inputsEl.children)];
      grid = toArr(appsEl.children);
    } else {
      rows = [toArr(pbar.children)];
      grid = toArr(pgrid.children);
    }
    for (var i = 0; i < grid.length; i += COLS) rows.push(grid.slice(i, i + COLS));
    rows = rows.filter(function (r) { return r.length; });
    // keep the focus on the same thing after a rebuild
    var again = null;
    if (moved || view !== 'home') rows.forEach(function (r) { r.forEach(function (e) { if (keyOf(e) === lastKey) again = e; }); });
    focus(again || first());
  }
  function first() {
    if (view !== 'home') return pgrid.firstChild || pbar.firstChild;
    return appsEl.firstChild || inputsEl.firstChild || bar.firstChild;
  }
  function where(e) {
    for (var r = 0; r < rows.length; r++) { var c = rows[r].indexOf(e); if (c >= 0) return { r: r, c: c }; }
    return null;
  }
  function centerX(e) { var b = e.getBoundingClientRect(); return b.left + b.width / 2; }
  function reveal(e) {
    var box = view === 'home' ? main : pbody;
    if (e.parentNode === bar || e.parentNode === inputsEl || e.parentNode === pbar) { box.scrollTop = 0; return; }
    var b = e.getBoundingClientRect(), m = box.getBoundingClientRect();
    if (b.bottom + 60 > m.bottom) box.scrollTop += b.bottom + 60 - m.bottom;
    else if (b.top - 20 < m.top) box.scrollTop -= m.top - (b.top - 20);
  }
  function focus(e) {
    if (!e) return;
    if (cur && cur !== e) cur.classList.remove('focus');
    cur = e;
    e.classList.add('focus');
    lastKey = keyOf(e);
    reveal(e);
  }
  function move(dr, dc) {
    moved = true;
    redArmed = null;
    var p = where(cur);
    if (!p) { focus(first()); return; }
    if (dc) { var row = rows[p.r]; focus(row[Math.max(0, Math.min(row.length - 1, p.c + dc))]); return; }
    var nr = p.r + dr;
    if (nr < 0 || nr >= rows.length) return;
    var x = centerX(cur), best = null, bd = 1e9;
    rows[nr].forEach(function (e) { var d = Math.abs(centerX(e) - x); if (d < bd) { bd = d; best = e; } });
    focus(best);
  }
  function owner(t) {
    while (t && t !== document.body) { if (t.kiro) return t; t = t.parentNode; }
    return null;
  }
  function inView(e) { return !!e && !!where(e); }
  document.addEventListener('keydown', function (ev) {
    var k = ev.keyCode;
    if (view === 'dialog') {
      if (k === 13 || k === 461 || k === 8) { closeAdd(); ev.preventDefault(); }
      return;
    }
    switch (k) {
      case 37: move(0, -1); break;
      case 39: move(0, 1); break;
      case 38: move(-1, 0); break;
      case 40: move(1, 0); break;
      case 13: activate(cur); break;
      case 403: redKey(); break;
      case 461: case 8: if (view === 'panel') closePanel(); break; // Back on Home does nothing
      default: return;
    }
    ev.preventDefault();
  });
  document.addEventListener('mouseover', function (ev) {
    var e = owner(ev.target);
    if (e && e !== cur && inView(e)) { moved = true; redArmed = null; focus(e); }
  });
  document.addEventListener('click', function (ev) {
    if (view === 'dialog') { closeAdd(); return; }
    var e = owner(ev.target);
    if (inView(e)) activate(e);
  });

  // ---- clock ----
  var DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
  var MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
  var clock = null;
  function tick() {
    var d = new Date(), h = d.getHours(), m = d.getMinutes();
    $('time').textContent = ((h % 12) || 12) + ':' + (m < 10 ? '0' : '') + m + (h < 12 ? ' AM' : ' PM');
    $('date').textContent = DAYS[d.getDay()] + ', ' + MONTHS[d.getMonth()] + ' ' + d.getDate();
  }
  function shown() {
    tick();
    clearInterval(clock);
    clock = setInterval(tick, 15000);
    refresh();
    applyBg(false);
    // pictures may have been added or removed while Home was hidden
    api('/bg', function (b) { if (b.returnValue && JSON.stringify(b) !== JSON.stringify(BGS)) setBgs(b, false); });
  }
  document.addEventListener('visibilitychange', function () {
    if (document.hidden) { clearInterval(clock); clearInterval(shuffleTimer); shuffleTimer = null; } else shown();
  });
  document.addEventListener('webOSRelaunch', shown);

  // ---- start: the saved catalog paints at once, live data replaces it when it arrives ----
  fill(bar, BAR, pill);
  showInputs(CAT.inputs || []);
  showApps(sortApps(CAT.apps || []));
  shown();
})();
