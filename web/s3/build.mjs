import { build } from 'esbuild';
import { mkdir, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
await mkdir('s3/dist',{recursive:true});
const output=await build({entryPoints:['s3/component.ts','s3/client.ts','s3/key.ts'],bundle:true,write:false,platform:'browser',format:'esm',target:'es2022',outdir:'s3/dist',legalComments:'inline'});
const manifest={profile:'s3-dev-local-v1',default_enabled:false,runtime_approved:false,files:[]};
for(const file of output.outputFiles){await writeFile(file.path,file.contents);manifest.files.push({name:file.path.split('/').pop(),sha256:createHash('sha256').update(file.contents).digest('hex'),bytes:file.contents.length});}
await writeFile('s3/dist/manifest.json',JSON.stringify(manifest,null,2)+'\n');
console.log(JSON.stringify(manifest,null,2));
