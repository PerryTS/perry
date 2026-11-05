import * as http from "node:http";
for (const prior of [false,true]) {
 const r: any = new http.ServerResponse({method:'GET'} as any); r.sendDate=false;
 const a=['one']; if(prior) r.setHeader('Before', 1);
 console.log('chain', r.writeHead(201, {'X-Test': a, Num: 23}) === r);
 console.log(prior, r.getHeader('x-test') === a, typeof r.getHeader('num'), r.getHeaderNames().join(','));
 console.log(JSON.stringify(r.getHeaders()));
 a.push('after'); console.log(r._header.includes('X-Test: one\r\n'), r._header.includes('after'));
 try { r.setHeader('late',1); } catch(e:any) { console.log(e.code); }
}
const r:any=new http.ServerResponse({method:'GET'} as any); r.sendDate=false;
const a=['initial'];r.setHeader('X-Test',a);a.push('before');r.writeHead(200);
console.log(r._header.includes('X-Test: before\r\n'),r.getHeader('x-test')===a);
