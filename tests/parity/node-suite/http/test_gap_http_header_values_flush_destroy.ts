import * as http from "node:http";
const r:any=new http.ServerResponse({method:'GET'} as any);r.sendDate=false;
const a=['live'];r.setHeader('X-Test',a);r.flushHeaders();
console.log(r.headersSent,r.getHeader('x-test')===a,r.getHeaders()['x-test']===a,r.getHeaderNames().join(','));
try{r.appendHeader('x-test','late')}catch(e:any){console.log(e.code)}
try{r.removeHeader('x-test')}catch(e:any){console.log(e.code)}
r.destroy(); console.log(r.destroyed,r.getHeader('x-test')===a,r.getHeaders()['x-test']===a,r.hasHeader('x-test'));
const d:any=new http.ServerResponse({method:'GET'} as any);d.setHeader('Old',a);d.destroy();
d.appendHeader('Old','after');d.setHeader('Num',23);d.removeHeader('num');console.log(d.getHeader('old')===a,a.join(','),d.getHeaderNames().join(','));
const e:any=new http.ServerResponse({method:'GET'} as any);const ended=['ended'];
e.setHeader('X-Test',ended);e.end('body');
console.log(e.getHeader('x-test')===ended,e.getHeaders()['x-test']===ended,e.getHeaderNames().join(','));
try{e.setHeader('late',1)}catch(err:any){console.log(err.code)}
