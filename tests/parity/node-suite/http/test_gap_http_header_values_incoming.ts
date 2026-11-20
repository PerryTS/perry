import http from 'node:http';
async function main(){
 const server=http.createServer((req,res)=>{
  res.sendDate=false;const a=['one'];res.setHeader('X-Test',a);res.setHeader('Num',23);
  res.setHeader('Set-Cookie',['a=1','b=2']);res.setHeader('Cookie',['c=3','d=4']);
  a.push('before');res.end('ok');
 });
 await new Promise<void>(resolve=>server.listen(0,'127.0.0.1',resolve));
 const address:any=server.address();
 await new Promise<void>((resolve,reject)=>{
  http.get('http://127.0.0.1:'+address.port+'/',res=>{
   console.log(typeof res.headers['x-test'],res.headers['x-test'],res.headers.num);
   console.log(Array.isArray(res.headers['set-cookie']),JSON.stringify(res.headers['set-cookie']),res.headers.cookie);
   const pairs:string[]=[];for(let i=0;i<res.rawHeaders.length;i+=2){
    if(['x-test','num','set-cookie','cookie'].includes(res.rawHeaders[i].toLowerCase()))pairs.push(res.rawHeaders[i]+': '+res.rawHeaders[i+1]);
   }
   console.log(JSON.stringify(pairs));res.resume();res.on('end',resolve);
  }).on('error',reject);
 });
 await new Promise<void>(resolve=>server.close(()=>resolve()));
}
main().catch(e=>{console.error(e);process.exitCode=1});
