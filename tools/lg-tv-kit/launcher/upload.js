#!/usr/bin/env node
// Phone upload page for the Kiro launcher's backgrounds. The launcher starts it ("Add pictures"); it listens on the home
// network only under a random code shown on the TV, accepts JPEG pictures only, and stops 15 minutes after its last use.
// The phone's browser resizes every picture to 1920x1080 (plus a small preview) before sending it, so the TV does no resizing.
'use strict';
const http = require('http');
const fs = require('fs');
const crypto = require('crypto');
const cp = require('child_process');

const CODE = process.argv[2] || '';
const H = '/media/developer/kiro/home';
const BG = H + '/app/bg';
const PORT = 8767;
const LIMIT = { full: 5 * 1024 * 1024, thumb: 400 * 1024 };
const MAX_PICTURES = 80;
const IDLE_MS = 15 * 60 * 1000;
const NAME = /^p[a-z0-9]{6,20}\.jpg$/;
// game files for RetroArch: kept outside RetroArch's own folder, so an update of RetroArch does not take them away
const GAMES = '/media/developer/games';
const GAME_MAX = 1024 * 1024 * 1024;
const GAME_RESERVE = 300 * 1024 * 1024; // always leave this much space free for the TV

if (!/^[a-z0-9]{5,12}$/.test(CODE)) {
  console.error('usage: node upload.js <code>');
  process.exit(2);
}
fs.mkdirSync(BG + '/thumbs', { recursive: true });
fs.mkdirSync(GAMES, { recursive: true });

// a file name the phone sent, made safe: no folders, no leading dot, only ordinary characters, and an extension
function cleanName(n) {
  n = String(n || '').split('/').pop().split('\\').pop();
  n = n.replace(/[^A-Za-z0-9 ._()\[\],'!&+-]/g, '').replace(/^[.\s]+/, '').slice(0, 120).trim();
  return /\.[A-Za-z0-9]{1,5}$/.test(n) ? n : '';
}
function uniquePath(dir, name) {
  const dot = name.lastIndexOf('.');
  const base = name.slice(0, dot), ext = name.slice(dot);
  let p = dir + '/' + name;
  for (let i = 2; fs.existsSync(p); i++) p = dir + '/' + base + ' (' + i + ')' + ext;
  return p;
}
function freeBytes(dir) {
  try {
    const f = cp.execFileSync('df', ['-k', dir]).toString().trim().split('\n').slice(1).join(' ').trim().split(/\s+/);
    return parseInt(f[3], 10) * 1024;
  } catch (e) {
    return 0;
  }
}
function receiveGame(req, res, url) {
  const name = cleanName(url.searchParams.get('name'));
  if (!name) return json(res, 400, { error: 'that file name or type is not supported' });
  const size = parseInt(req.headers['content-length'] || '0', 10);
  if (!size) return json(res, 411, { error: 'the phone did not say how big the file is' });
  if (size > GAME_MAX) return json(res, 413, { error: 'too big (1 GB at most)' });
  const free = freeBytes(GAMES);
  if (size > free - GAME_RESERVE) {
    return json(res, 507, { error: 'not enough space on the TV (' + Math.max(0, Math.floor((free - GAME_RESERVE) / 1048576)) + ' MB usable)' });
  }
  const final = uniquePath(GAMES, name);
  const tmp = GAMES + '/.upload-' + crypto.randomBytes(4).toString('hex') + '.part';
  const out = fs.createWriteStream(tmp);
  let got = 0, failed = false;
  const clean = () => fs.unlink(tmp, () => {});
  req.on('data', c => {
    got += c.length;
    touch();
    if (got > size) { failed = true; req.destroy(); }
  });
  req.on('aborted', () => { failed = true; out.destroy(); clean(); });
  out.on('error', e => { failed = true; clean(); if (!res.headersSent) json(res, 500, { error: 'could not save it (' + e.code + ')' }); });
  out.on('finish', () => {
    if (failed || got !== size) { clean(); if (!res.headersSent) json(res, 400, { error: 'the upload did not finish' }); return; }
    fs.rename(tmp, final, e => {
      if (e) { clean(); return json(res, 500, { error: 'could not save it (' + e.code + ')' }); }
      json(res, 200, { name: final.split('/').pop() });
    });
  });
  req.pipe(out);
}

let idle;
function touch() {
  clearTimeout(idle);
  idle = setTimeout(stop, IDLE_MS);
}
function stop() {
  try { fs.unlinkSync('/tmp/kiro-upload.url'); } catch (e) {}
  process.exit(0);
}
function reply(res, status, type, body) {
  res.writeHead(status, { 'Content-Type': type, 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' });
  res.end(body);
}
function json(res, status, obj) {
  reply(res, status, 'application/json', JSON.stringify(obj));
}
function readBody(req, max, cb) {
  const parts = [];
  let size = 0;
  let done = false;
  const finish = b => { if (!done) { done = true; cb(b); } };
  req.on('data', c => {
    size += c.length;
    if (size > max) { finish(null); req.destroy(); } else parts.push(c);
  });
  req.on('end', () => finish(Buffer.concat(parts)));
  req.on('error', () => finish(null));
}
const isJpeg = b => !!b && b.length > 1000 && b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff;
function count() {
  try { return fs.readdirSync(BG).filter(n => NAME.test(n)).length; } catch (e) { return 0; }
}
function save(file, data) {
  fs.writeFileSync(file + '.part', data);
  fs.renameSync(file + '.part', file);
}

const PAGE = `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>TV backgrounds</title><style>
body{font-family:-apple-system,system-ui,sans-serif;background:#0b0f17;color:#eef1f6;margin:0;padding:24px}
h1{font-size:26px;margin:0 0 8px}p{color:#aab3c2;line-height:1.4}
.btn{display:block;background:#2f6df6;color:#fff;text-align:center;padding:18px;border-radius:14px;font-size:20px;margin:20px 0}
#pick{display:none}.opt{display:flex;gap:10px;align-items:center;font-size:17px;margin:8px 0}
#list div{display:flex;align-items:center;gap:12px;margin:10px 0;font-size:16px}
#list img{width:96px;height:54px;object-fit:cover;border-radius:6px;background:#222;flex:none}
.ok{color:#7bd88f}.err{color:#ff8a80}
</style></head><body>
<h1>Backgrounds</h1>
<p>Pick one or more pictures. This phone resizes each one for the TV, then sends it over your Wi-Fi. Wide (landscape) pictures look best.</p>
<label class="opt"><input type="checkbox" id="fit"> Show the whole picture (black bars) instead of filling the screen</label>
<label class="btn">Choose pictures<input type="file" id="pick" accept="image/*" multiple></label>
<div id="list"></div>
<h1 style="margin-top:36px">Games for RetroArch</h1>
<p>Pick game files from the Files app (zip and 7z work too). They go to the Games folder on the TV: in RetroArch, open Load Content and pick the Games folder. For a disc game, send the .cue and every .bin.</p>
<label class="btn" style="background:#107c10">Choose game files<input type="file" id="gpick" multiple style="display:none"></label>
<div id="glist"></div>
<script>
var code = location.pathname.split('/')[1];
var pick = document.getElementById('pick'), list = document.getElementById('list');
pick.onchange = function () { var f = Array.prototype.slice.call(pick.files); pick.value = ''; run(f, 0); };
function run(f, i) { if (i < f.length) send(f[i], function () { run(f, i + 1); }); }
function load(file, cb) {
  var u = URL.createObjectURL(file), img = new Image();
  img.onload = function () { cb(img, u); };
  img.onerror = function () { cb(null, u); };
  img.src = u;
}
function draw(img, w, h, q, fit, cb) {
  var c = document.createElement('canvas'); c.width = w; c.height = h;
  var x = c.getContext('2d'); x.fillStyle = '#000'; x.fillRect(0, 0, w, h);
  var iw = img.naturalWidth, ih = img.naturalHeight;
  var s = fit ? Math.min(w / iw, h / ih) : Math.max(w / iw, h / ih);
  x.imageSmoothingQuality = 'high';
  x.drawImage(img, (w - iw * s) / 2, (h - ih * s) / 2, iw * s, ih * s);
  c.toBlob(cb, 'image/jpeg', q);
}
function post(path, blob, cb) {
  var r = new XMLHttpRequest();
  r.open('POST', '/' + code + path);
  r.onload = function () { var j = {}; try { j = JSON.parse(r.responseText); } catch (e) {} cb(r.status === 200 ? null : (j.error || ('error ' + r.status)), j); };
  r.onerror = function () { cb('the TV did not answer. Is this phone on the same Wi-Fi as the TV?'); };
  r.send(blob);
}
var gpick = document.getElementById('gpick'), glist = document.getElementById('glist');
gpick.onchange = function () { var f = Array.prototype.slice.call(gpick.files); gpick.value = ''; runGames(f, 0); };
function runGames(f, i) { if (i < f.length) sendGame(f[i], function () { runGames(f, i + 1); }); }
function sendGame(file, next) {
  var row = document.createElement('div'), txt = document.createElement('span');
  row.appendChild(txt); glist.appendChild(row);
  txt.textContent = 'Sending ' + file.name + '...';
  var r = new XMLHttpRequest();
  r.open('POST', '/' + code + '/game?name=' + encodeURIComponent(file.name));
  r.upload.onprogress = function (e) { if (e.lengthComputable) txt.textContent = 'Sending ' + file.name + ': ' + Math.round(100 * e.loaded / e.total) + '%'; };
  r.onload = function () {
    var j = {}; try { j = JSON.parse(r.responseText); } catch (e) {}
    if (r.status === 200) { txt.className = 'ok'; txt.textContent = 'On the TV: ' + (j.name || file.name); }
    else { txt.className = 'err'; txt.textContent = file.name + ': ' + (j.error || ('error ' + r.status)); }
    next();
  };
  r.onerror = function () { txt.className = 'err'; txt.textContent = file.name + ': the TV did not answer. Is this phone on the same Wi-Fi as the TV?'; next(); };
  r.send(file);
}
function send(file, next) {
  var row = document.createElement('div'), pic = document.createElement('img'), txt = document.createElement('span');
  txt.textContent = 'Preparing ' + file.name + '...'; row.appendChild(pic); row.appendChild(txt); list.appendChild(row);
  var fit = document.getElementById('fit').checked;
  var bad = function (m) { txt.className = 'err'; txt.textContent = file.name + ': ' + m; next(); };
  load(file, function (img, u) {
    if (!img) return bad('this phone cannot open that picture');
    pic.src = u;
    draw(img, 1920, 1080, 0.88, fit, function (full) {
      draw(img, 384, 216, 0.8, fit, function (thumb) {
        if (!full || !thumb) return bad('could not resize it');
        txt.textContent = 'Sending ' + file.name + '...';
        post('/full', full, function (err, j) {
          if (err) return bad(err);
          post('/thumb?name=' + encodeURIComponent(j.name), thumb, function (err2) {
            if (err2) return bad(err2);
            txt.className = 'ok'; txt.textContent = 'On the TV: ' + file.name; next();
          });
        });
      });
    });
  });
}
</script></body></html>`;

const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://tv');
  const parts = url.pathname.split('/').filter(Boolean);
  if (parts[0] !== CODE) return reply(res, 404, 'text/plain', 'Not found');
  touch();
  const what = parts[1] || '';
  if (req.method === 'GET' && what === '') return reply(res, 200, 'text/html; charset=utf-8', PAGE);
  if (req.method === 'POST' && what === 'full') {
    if (count() >= MAX_PICTURES) return json(res, 507, { error: 'the TV already has ' + MAX_PICTURES + ' pictures; delete some first' });
    return readBody(req, LIMIT.full, b => {
      if (!isJpeg(b)) return json(res, 400, { error: 'not a JPEG picture, or too big' });
      const name = 'p' + Date.now().toString(36) + crypto.randomBytes(3).toString('hex') + '.jpg';
      try { save(BG + '/' + name, b); } catch (e) { return json(res, 500, { error: 'could not save it (' + e.code + ')' }); }
      json(res, 200, { name });
    });
  }
  if (req.method === 'POST' && what === 'thumb') {
    const name = url.searchParams.get('name') || '';
    if (!NAME.test(name) || !fs.existsSync(BG + '/' + name)) return json(res, 400, { error: 'unknown picture' });
    return readBody(req, LIMIT.thumb, b => {
      if (!isJpeg(b)) return json(res, 400, { error: 'not a JPEG picture, or too big' });
      try { save(BG + '/thumbs/' + name, b); } catch (e) { return json(res, 500, { error: 'could not save it (' + e.code + ')' }); }
      cp.execFile('/bin/sh', [H + '/kiro-bg.sh', 'added', name], () => json(res, 200, { ok: true }));
    });
  }
  if (req.method === 'POST' && what === 'game') return receiveGame(req, res, url);
  if (what === 'done') {
    json(res, 200, { ok: true });
    return setTimeout(stop, 500);
  }
  reply(res, 404, 'text/plain', 'Not found');
});
server.on('error', e => { console.error(e.message); process.exit(1); });
server.listen(PORT, '0.0.0.0', () => { console.log(new Date().toISOString() + ' upload page on port ' + PORT); touch(); });
