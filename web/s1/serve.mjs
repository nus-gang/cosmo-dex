// Loopback-only same-origin development proxy. No signing or private-key endpoint.
import http from 'node:http';
import { readFile } from 'node:fs/promises';
const upstream = new URL(process.env.S1_API ?? 'http://127.0.0.1:8787');
if (upstream.protocol !== 'http:' || !['127.0.0.1', 'localhost'].includes(upstream.hostname) || upstream.username || upstream.password) throw Error('loopback HTTP API required');
const port = Number(process.env.S1_PORT ?? 8080);
const origin = `http://127.0.0.1:${port}`;
const server = http.createServer(async (req, res) => {
  res.setHeader('Cache-Control', 'no-store'); res.setHeader('X-Content-Type-Options', 'nosniff'); res.setHeader('X-Frame-Options', 'DENY');
  try {
    if (req.headers.host !== `127.0.0.1:${port}` || (req.headers.origin && req.headers.origin !== origin)) { res.writeHead(403).end(); return; }
    const path = req.url ?? '/';
    if (req.method === 'GET' && ['/', '/wallet.js'].includes(path)) { res.setHeader('Content-Type', path === '/' ? 'text/html; charset=utf-8' : 'text/javascript'); res.end(await readFile(`dist/s1/${path === '/' ? 'index.html' : 'wallet.js'}`)); return; }
    const getAllowed = /^\/s1\/(network|accounts\/nus1[a-z0-9]+(?:\/requests\/[0-9a-f]{64})?|txs\/[A-F0-9]{64})$/.test(path);
    const postAllowed = req.method === 'POST' && path === '/s1/txs' && req.headers.origin === origin && req.headers['content-type'] === 'application/json';
    if (!(req.method === 'GET' && getAllowed) && !postAllowed) { res.writeHead(404).end(); return; }
    let body = ''; for await (const part of req) { body += part; if (body.length > 24000) { res.writeHead(413).end(); return; } }
    if (postAllowed) { const value = JSON.parse(body); if (Object.keys(value).join() !== 'tx_bytes' || typeof value.tx_bytes !== 'string' || !/^[A-Za-z0-9+/]+={0,2}$/.test(value.tx_bytes)) { res.writeHead(400).end(); return; } }
    const r = await fetch(new URL(path, upstream), { method: req.method, headers: { 'Content-Type': 'application/json' }, ...(postAllowed ? { body } : {}), signal: AbortSignal.timeout(12000) });
    res.writeHead(r.status, { 'Content-Type': 'application/json' }); res.end(await r.text());
  } catch { res.writeHead(503, { 'Content-Type': 'application/json' }); res.end('{"state":"SUBMISSION_UNKNOWN"}'); }
});
server.listen(port, '127.0.0.1', () => console.log(`S1 wallet: ${origin}`));
