// Original values feed the wire; IncomingMessage retains Node's duplicate rules.
import http from 'node:http';
import net from 'node:net';
async function exchange(port:number, path:string):Promise<string> {
 return new Promise((resolve,reject)=>{
  const socket=net.connect(port,'127.0.0.1');let raw='';
  socket.on('connect',()=>socket.write('GET /'+path+' HTTP/1.1\r\nHost: localhost\r\nX-Input: one\r\nX-Input: two\r\nCookie: a=1\r\nCookie: b=2\r\nSet-Cookie: c=3\r\nSet-Cookie: d=4\r\nConnection: close\r\n\r\n'));
  socket.on('data',c=>raw+=c.toString());socket.on('end',()=>resolve(raw));socket.on('error',reject);
 });
}
async function main(){
 const server=http.createServer((req,res)=>{
  res.sendDate=false;
  console.log(req.url,JSON.stringify(req.headers['x-input']),JSON.stringify(req.headers.cookie),JSON.stringify(req.headers['set-cookie']));
  console.log(req.rawHeaders.join('|'));
  const a=['one'];res.setHeader('X-Test',a);res.setHeader('Num',23);res.setHeader('Cookie',['e=5','f=6']);res.setHeader('Set-Cookie',['a=1','b=2']);a.push('before');
  if(req.url==='/flush')res.flushHeaders();else res.writeHead(200);
  a.push('after');res.end('ok');
 });
 await new Promise<void>(resolve=>server.listen(0,'127.0.0.1',resolve));
 const address:any=server.address();
 for(const path of ['writehead','flush']){
  const raw=await exchange(address.port,path);
  const headers=raw.split('\r\n').filter(line=>/^(X-Test|Num|Cookie|Set-Cookie):/.test(line));
  console.log(path,JSON.stringify(headers));
 }
 await new Promise<void>(resolve=>server.close(()=>resolve()));
}
main().catch(e=>{console.error(e);process.exitCode=1});
