import * as http from 'node:http';
const r:any=new http.ServerResponse({method:'GET'} as any);
const a=['one'];const m=new Map<string,any>([['X-Test',a],['Num',23]]);
console.log(r.setHeaders(m)===r,r.getHeader('x-test')===a,typeof r.getHeader('num'));
a.push('two');console.log(JSON.stringify(r.getHeaders()));
const h=new Headers([['x-next','ok'],['set-cookie','a=1'],['set-cookie','b=2']]);
r.setHeaders(h);console.log(r.getHeader('x-next'),JSON.stringify(r.getHeader('set-cookie')));
try{r.setHeaders({x:1})}catch(e:any){console.log(e.code)}
