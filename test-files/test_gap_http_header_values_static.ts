// Inferred native receivers exercise static ABI lowering as well as any dispatch.
import * as http from 'node:http';
const r = new http.ServerResponse({method:'GET'} as any);
const a=['one'];
console.log(r.setHeader('X-Test',a)===r,r.getHeader('x-test')===a);
console.log(r.appendHeader('Num',23)===r,typeof r.getHeader('num'));
r.appendHeader('Num',24);
console.log(JSON.stringify(r.getHeaders()),r.getHeaderNames().join(','));
console.log(r.writeHead(201)===r,r.getHeader('x-test')===a,r.headersSent);
const o = new http.OutgoingMessage();
o.setHeader('X-Test',a);o.appendHeader('X-Test','two');
console.log(o.getHeader('x-test')===a,JSON.stringify(o.getHeaders()));
