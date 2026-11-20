import * as http from 'node:http';
const r:any=new http.ServerResponse({method:'GET'} as any);const a=['one'];
console.log(r.getHeader('missing')===undefined);
r.setHeader('X-Test',a);r.setHeader('Num',23);r.setHeader('Text','ok');
console.log(r.getHeader('X-TEST')===a,typeof r.getHeader('num'),r.getHeader('num'),r.getHeader('text'));
a.push('two');console.log(r.getHeader('x-test').join(','));
