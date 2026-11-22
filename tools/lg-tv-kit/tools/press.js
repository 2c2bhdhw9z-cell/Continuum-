// Test helper (runs on the TV, only when called): presses remote keys by writing key events to the remote's input device,
// the same events the LG remote driver produces. Userspace on this TV is 32-bit, so an input_event is 16 bytes.
//   node press.js /dev/input/event1 right right enter
'use strict';
const fs = require('fs');
const KEYS = { up: 103, down: 108, left: 105, right: 106, enter: 28, back: 158, home: 102, lghome: 773, exit: 174 };
const [dev, ...names] = process.argv.slice(2);
const fd = fs.openSync(dev, 'w');
function ev(type, code, value) {
  const b = Buffer.alloc(16);
  const t = Date.now();
  b.writeInt32LE(Math.floor(t / 1000), 0);
  b.writeInt32LE((t % 1000) * 1000, 4);
  b.writeUInt16LE(type, 8);
  b.writeUInt16LE(code, 10);
  b.writeInt32LE(value, 12);
  fs.writeSync(fd, b);
}
const sleep = ms => new Promise(r => setTimeout(r, ms));
(async () => {
  for (const n of names) {
    const m = /^wait(\d+)$/.exec(n);
    if (m) { await sleep(Number(m[1])); continue; }
    const code = KEYS[n];
    if (!code) throw new Error('unknown key ' + n);
    ev(1, code, 1); ev(0, 0, 0);
    await sleep(60);
    ev(1, code, 0); ev(0, 0, 0);
    await sleep(350);
  }
  fs.closeSync(fd);
})();
