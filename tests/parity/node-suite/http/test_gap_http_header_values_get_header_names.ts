import * as http from 'node:http';
const r:any=new http.ServerResponse({method:'GET'} as any);
console.log(r.getHeaderNames().length);r.setHeader('Z-First',1);r.setHeader('A-Second',2);r.setHeader('Z-FIRST',3);
console.log(r.getHeaderNames().join(','));r.removeHeader('z-first');r.setHeader('Z-First',4);
console.log(r.getHeaderNames().join(','));const names=r.getHeaderNames();names.push('other');console.log(r.getHeaderNames().join(','));
const indexed:any=new http.ServerResponse({method:'GET'} as any);
for(const name of ['z','10','2','01','4294967295','0','a']) indexed.setHeader(name,name);
console.log(indexed.getHeaderNames().join(','),Object.keys(indexed.getHeaders()).join(','));
indexed.removeHeader('2');indexed.removeHeader('z');indexed.removeHeader('a');indexed.setHeader('2','again');
console.log(indexed.getHeaderNames().join(','),indexed.getHeader('10'),indexed.getHeader('2'));
indexed.destroy();indexed.removeHeader('01');indexed.removeHeader('0');indexed.setHeader('tail',7);
console.log(indexed.getHeaderNames().join(','),indexed.getHeader('10'),indexed.getHeader('tail'),Object.keys(indexed.getHeaders()).join(','));

for(let i=0;i<32;i++)indexed.setHeader('extra-'+i,i);
indexed.removeHeader('extra-15');indexed.removeHeader('extra-0');
console.log(indexed.getHeader('extra-31'),indexed.getHeader('extra-16'),indexed.hasHeader('extra-15'),indexed.getHeaderNames().length,Object.keys(indexed.getHeaders()).length);
