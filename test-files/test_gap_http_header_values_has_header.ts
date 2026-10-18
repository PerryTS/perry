import * as http from 'node:http';
const r:any=new http.ServerResponse({method:'GET'} as any);
console.log(r.hasHeader('missing'),r.hasHeader('constructor'),r.hasHeader('__proto__'));
r.setHeader('X-Empty',[]);r.setHeader('Num',0);r.setHeader('constructor','own');r.setHeader('__proto__',['own']);
console.log(r.hasHeader('X-EMPTY'),r.hasHeader('NUM'),r.hasHeader('Constructor'),r.hasHeader('__proto__'));
r.removeHeader('x-empty');console.log(r.hasHeader('x-empty'),r.getHeader('constructor'),r.getHeader('__proto__').join(','));
