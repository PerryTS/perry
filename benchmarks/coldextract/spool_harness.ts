// Harness around verbatim createSpool and original createWriter.writePart.
const ROOT='/root/lanes/perry-coldextract/micro';
const rows=JSON.parse(builtin.fs.readFileSync(ROOT+'/files.json','utf8')).filter((f:any)=>f.size>=STREAM_FILE);
const payload=builtin.fs.readFileSync(ROOT+'/payload.bin');
const dir='/root/lanes/perry-coldextract/work/micro-spool-'+process.pid;
const writer=createWriter(dir,{blocking:true});
let count=0,bytes=0;
async function main(){
 for(let round=0;round<Number(process.argv[2] ?? '5');round++){
  const spool=createSpool(dir+'/files');
  const files:any[]=[];
  for(const f of rows){
   const data=new Uint8Array(payload.buffer,payload.byteOffset+f.at,f.size);
   const write=spool.open(f.path,0,f.size)!;
   for(let at=0;at<data.length;at+=1024*1024)write(data.subarray(at,at+1024*1024));
   const result=spool.close();
   if(result.hash!==f.integrity)throw new Error('spool hash mismatch');
   const blob=writer.contentPath(result.hash,f.exec).slice((dir+'/files/').length);
   files.push({path:f.path,exec:f.exec,chunk:-1,at:0,size:f.size,temp:result.temp,blob});
   count++;bytes+=data.length;
  }
  await writer.writePart({data:[],files},false);
  spool.drop(()=>true);
 }
 console.log(count,bytes);
}
main();
