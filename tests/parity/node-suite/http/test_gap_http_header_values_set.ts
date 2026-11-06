import * as http from "node:http";
const r: any = new http.ServerResponse({method:'GET'} as any);
const a = ['a', 'b'];
console.log(r.setHeader('X-Test', a) === r, r.getHeader('x-test') === a, r.getHeader('X-TEST').join(','));
r.setHeader('Num', 23); r.setHeader('Text', 'ok');
console.log(typeof r.getHeader('num'), r.getHeader('num'), typeof r.getHeader('text'));
a.push('c'); console.log(r.getHeader('x-test') === a, r.getHeader('x-test').join(','));
r.setHeader('X-Test', 'changed'); console.log(r.getHeader('x-test'), a.join(','));
