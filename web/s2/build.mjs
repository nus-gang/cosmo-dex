import { build } from 'esbuild';
import { mkdir, copyFile } from 'node:fs/promises';
await mkdir('dist/s2',{recursive:true});
await build({entryPoints:['s2/browser.ts'],bundle:true,platform:'browser',format:'iife',target:'es2022',outfile:'dist/s2/wallet.js',legalComments:'inline'});
await copyFile('s2/index.html','dist/s2/index.html');
