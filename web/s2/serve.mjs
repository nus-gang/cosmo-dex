// Static loopback preview only. API remains at the approved CORS endpoint.
import http from 'node:http';
import { readFile } from 'node:fs/promises';
const server=http.createServer(async(req,res)=>{
  res.setHeader('Cache-Control','no-store'); res.setHeader('X-Content-Type-Options','nosniff'); res.setHeader('X-Frame-Options','DENY');
  if(req.headers.host!=='127.0.0.1:5173'||req.method!=='GET'||!['/','/wallet.js'].includes(req.url)){res.writeHead(403).end();return;}
  try {res.setHeader('Content-Type',req.url==='/'?'text/html; charset=utf-8':'text/javascript');res.end(await readFile(new URL(req.url==='/'?'../dist/s2/index.html':'../dist/s2/wallet.js',import.meta.url)));}
  catch {res.writeHead(503).end('Build the S2 wallet first.');}
});
server.listen(5173,'127.0.0.1');
