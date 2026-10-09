import { builtin } from './builtin.ts';
import { createWriter, verifyTarball } from './unpack.ts';
import { createHasher, concat, toBase64 } from './runtime.ts';
import { hashOf, createVerifier, parseIntegrity } from './integrity.ts';
const ROOT='/root/lanes/perry-coldextract/micro';
const mode=process.argv[2] ?? 'hash';
const rounds=Number(process.argv[3] ?? '5');
const fs=builtin.fs;
const archives=JSON.parse(fs.readFileSync(ROOT+'/archives.json','utf8'));
const files=JSON.parse(fs.readFileSync(ROOT+'/files.json','utf8'));
const payload=fs.readFileSync(ROOT+'/payload.bin');
const windows=files.map((f:any)=>new Uint8Array(payload.buffer,payload.byteOffset+f.at,f.size));
const tarballs=archives.map((a:any)=>fs.readFileSync(a.path));
const chunks=tarballs.map((b:Uint8Array)=>{
 const blocks:Uint8Array[]=[];
 for(let at=0;at<b.length;at+=1024*1024) blocks.push(b.subarray(at,at+1024*1024));
 return blocks;
});
let count=0,bytes=0,check='';
async function main(){
 for(let r=0;r<rounds;r++){
  if(mode==='noop'){count++;}
  else if(mode==='write-part'){
   const dir='/root/lanes/perry-coldextract/work/micro-'+process.pid+'-'+r;
   const writer=createWriter(dir,{blocking:true});
   const part={data:[payload.buffer],files:files.map((f:any)=>({path:f.path,exec:f.exec,chunk:0,at:payload.byteOffset+f.at,size:f.size}))};
   const blobs=await writer.writePart(part,false);
   count+=blobs.length;bytes+=payload.length;check=blobs[blobs.length-1];
  }else if(mode==='hash' || mode==='hash-small'){
   for(let i=0;i<chunks.length;i++){
    const hash=createHasher('sha512');
    if(mode==='hash') for(const chunk of chunks[i]) hash.update(chunk);
    else for(const chunk of chunks[i]) for(let at=0;at<chunk.length;at+=16384)hash.update(chunk.subarray(at,at+16384));
    const actual='sha512-'+toBase64(await hash.digest());
    if(actual!==archives[i].integrity)throw new Error('hash mismatch');
    count++;bytes+=tarballs[i].length;check=actual;
   }
  }else if(mode==='file-hash'){
   for(let i=0;i<windows.length;i++){
    const hash=await hashOf(windows[i]);
    if(hash!==files[i].integrity)throw new Error('file hash mismatch');
    count++;bytes+=windows[i].length;check=hash;
   }
  }else if(mode==='verify'){
   for(let i=0;i<chunks.length;i++){
    await verifyTarball(archives[i].integrity,chunks[i]);count++;bytes+=tarballs[i].length;
   }
  }else if(mode==='verdict'){
   // End step, without hashing payload: upm's original verifier, empty digest.
   const empty='sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg==';
   for(let i=0;i<1000;i++){const v=createVerifier(empty);await v.verify();await v.verify();count++;}
  }else if(mode==='parse-integrity'){
   for(let i=0;i<1000;i++)for(const a of archives){check=parseIntegrity(a.integrity).digest;count++;}
  }else if(mode==='copies'){
   for(const blocks of chunks){const b=concat(blocks); const out=new Uint8Array(b.length);out.set(b);count++;bytes+=out.length;check=String(out[0])+':'+String(out[out.length-1]);}
  }else if(mode==='views'){
   for(const data of windows){check=toBase64(data.subarray(0,64));count++;bytes+=data.length;}
  }else if(mode==='write-sync' || mode==='write-async'){
   const dir='/root/lanes/perry-coldextract/work/micro-'+process.pid+'-'+r;
   fs.mkdirSync(dir,{recursive:true});
   const writer=createWriter(dir);
   const made=new Set<string>();
   for(let i=0;i<windows.length;i++){
    const file=dir+'/shard-'+(i%256)+'/file-'+i;
    if(mode==='write-async'){await writer.ensureDir(builtin.path.dirname(file));await writer.put(file,windows[i],files[i].exec?0o555:0o444);}
    else {const parent=builtin.path.dirname(file);if(!made.has(parent)){fs.mkdirSync(parent,{recursive:true});made.add(parent);}fs.writeFileSync(file,windows[i],{mode:files[i].exec?0o555:0o444,flag:'wx'});}
    count++;bytes+=windows[i].length;
   }
   // Cleanup outside the subprocess by the measurement controller.
  }else throw new Error(mode);
 }
 console.log(count,bytes,check);
}
main();
