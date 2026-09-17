#!/usr/bin/env node
/**
 * Zero-dependency static server for `web/`.
 *
 * Exists because two details matter and `python -m http.server` gets one of them
 * wrong:
 *
 *   - `.wasm` must be served as `application/wasm`, or
 *     `WebAssembly.instantiateStreaming` refuses the response and the wasm-bindgen
 *     glue silently falls back to the slower `ArrayBuffer` path.
 *   - `.mjs`/`.js` must be `text/javascript`, since the whole UI is ES modules
 *     loaded without a bundler.
 *
 * Usage: node scripts/serve.mjs [port]
 */

import { createServer } from 'node:http';
import { createReadStream } from 'node:fs';
import { stat } from 'node:fs/promises';
import { extname, join, normalize, resolve } from 'node:path';

const ROOT = resolve(new URL('../web', import.meta.url).pathname);
const PORT = Number(process.argv[2] ?? 8080);

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.webmanifest': 'application/manifest+json; charset=utf-8',
  '.wasm': 'application/wasm',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.map': 'application/json; charset=utf-8',
  '.ts': 'text/plain; charset=utf-8',
};

const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, `http://${req.headers.host}`);
    let pathname = decodeURIComponent(url.pathname);
    if (pathname.endsWith('/')) pathname += 'index.html';

    // Contain path traversal: normalise, then verify the result is still inside ROOT.
    const filePath = join(ROOT, normalize(pathname).replace(/^(\.\.[/\\])+/, ''));
    if (!filePath.startsWith(ROOT)) {
      res.writeHead(403).end('Forbidden');
      return;
    }

    const info = await stat(filePath).catch(() => null);
    if (!info || !info.isFile()) {
      res.writeHead(404, { 'content-type': 'text/plain' }).end('Not found');
      return;
    }

    res.writeHead(200, {
      'content-type': MIME[extname(filePath)] ?? 'application/octet-stream',
      'content-length': info.size,
      // No caching in development: a stale core or wasm module during a rebuild is
      // an hour of confusion.
      'cache-control': 'no-cache',
    });
    createReadStream(filePath).pipe(res);
  } catch (err) {
    res.writeHead(500, { 'content-type': 'text/plain' }).end(String(err));
  }
});

server.listen(PORT, () => {
  console.log(`serving ${ROOT} at http://localhost:${PORT}/`);
});
