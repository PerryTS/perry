import * as http from "node:http";
const r: any = new http.ServerResponse({method:'GET'} as any);
const a = ['a']; r.setHeader('X-Test', a);
console.log(r.appendHeader('x-test', ['b','c']) === r, r.getHeader('x-test') === a, a.join(','));
r.appendHeader('Num', 7); console.log(typeof r.getHeader('num'), r.getHeader('num'));
r.appendHeader('NUM', 8); console.log(Array.isArray(r.getHeader('num')), r.getHeader('num').join(','));
const cookies = ['a=1']; r.appendHeader('Set-Cookie', cookies); console.log(r.getHeader('set-cookie') === cookies);
r.appendHeader('set-cookie', 'b=2'); console.log(cookies.join(','));

const grown=['start'];r.setHeader('X-Grown',grown);
for(let i=0;i<32;i++)r.appendHeader('X-Grown','v'+i);
console.log(r.getHeader('x-grown')===grown,grown.length,grown[32]);
