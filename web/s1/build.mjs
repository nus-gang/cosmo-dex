import { build } from 'esbuild';
import { mkdir, copyFile } from 'node:fs/promises';
await mkdir('dist/s1', { recursive: true });
await build({ entryPoints: ['s1/browser.ts'], bundle: true, platform: 'browser', format: 'iife', target: 'es2022', outfile: 'dist/s1/wallet.js', legalComments: 'inline' });
await copyFile('s1/index.html', 'dist/s1/index.html');
