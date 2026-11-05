import * as http from 'node:http';
const r:any=new http.ServerResponse({method:'GET'} as any);const a=['one'];
r.setHeader('X-Test',a);r.setHeader('Keep',23);
console.log(r.removeHeader('X-TEST')===undefined,r.getHeader('x-test')===undefined,r.hasHeader('x-test'),r.getHeader('keep'));
r.setHeader('X-Test',a);console.log(r.getHeader('x-test')===a,r.getHeaderNames().join(','));
r.removeHeader('missing');console.log(r.getHeaderNames().join(','));

const dated:any=new http.ServerResponse({method:'GET'} as any);
dated.setHeader('Date','explicit');dated.removeHeader('DATE');
console.log(dated.sendDate,dated.hasHeader('date'),dated.getHeaderNames().length);
