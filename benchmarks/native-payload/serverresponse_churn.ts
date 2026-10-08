import * as http from 'node:http';
import * as util from 'node:util';
import { Writable } from 'node:stream';
declare function gc(): void;
const H:any=http;
function Response(this:any, req:any) {
  H.ServerResponse.call(this,req);
  this.payload = new Array(256).fill(0);
  this.once('close',()=>{this.payload[0]=1;});
  this.once('error',()=>{this.payload[0]=2;});
  const socket=new Writable({write(chunk:any,enc:any,cb:any){cb();}});
  socket.on('error',()=>{this.payload[0]=3;});
  this.assignSocket(socket);
}
util.inherits(Response,H.ServerResponse);
const weak:WeakRef<any>[]=[];
let count=0;
function batch() {
  for(let i=0;i<2000;i++) {
    const r:any=new (Response as any)({method:'GET'});
    r.setHeader('x-i',String(i));
    if(i%200===0)weak.push(new WeakRef(r));
  }
  count+=2000;
  setTimeout(()=>{
    gc();
    setTimeout(()=>{
      gc();
      if(count===2000||count===20000) console.log('rss '+count+' '+process.memoryUsage().rss);
      if(count<20000) batch();
      else console.log('live '+weak.filter(w=>w.deref()!==undefined).length+' tracked '+weak.length);
    },0);
  },0);
}
batch();
