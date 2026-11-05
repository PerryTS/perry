import * as fs from 'node:fs';
import * as fsp from 'node:fs/promises';
const dir=fs.mkdtempSync('perry-writefile-byte-');
const source=Buffer.from('prefix-616263-suffix');
const window=source.subarray(7,13);
const typed=new Uint8Array(source.buffer,source.byteOffset+7,6);
const view=new DataView(source.buffer,source.byteOffset+7,6);
async function main(){
 try {
  for(const [i,bytes] of [window,typed,view].entries()){
   const path=dir+'/'+i;
   fs.writeFileSync(path,bytes,{encoding:'hex',mode:0o444,flag:'wx'});
   console.log('sync',fs.readFileSync(path,'utf8'));
   await fsp.writeFile(path+'-promise',bytes,{encoding:'base64',mode:0o444,flag:'wx'});
   console.log('promise',fs.readFileSync(path+'-promise','utf8'));
   await new Promise<void>((resolve,reject)=>fs.writeFile(path+'-callback',bytes,{encoding:'hex'},(error)=>error?reject(error):resolve()));
   console.log('callback',fs.readFileSync(path+'-callback','utf8'));
   try {fs.writeFileSync(path,bytes,{flag:'wx'});}catch(error:any){console.log('exclusive',error.code);}
  }
  const fd=fs.openSync(dir+'/0','r');
  try {fs.writeFileSync(fd,window);}catch(error:any){console.log('fd error',error.code,error.syscall);}
  fs.closeSync(fd);
  try {await fsp.writeFile(dir+'/missing/file',view);}catch(error:any){console.log('path error',error.code,error.syscall);}
  console.log('source',source.toString());
 }finally{fs.rmSync(dir,{recursive:true,force:true});}
}
main();
