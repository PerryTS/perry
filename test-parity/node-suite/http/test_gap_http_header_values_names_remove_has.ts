import * as http from "node:http";
const r: any = new http.ServerResponse({method:'GET'} as any);
console.log(r.getHeader('missing') === undefined, r.hasHeader('missing'), r.getHeaderNames().length);
r.setHeader('Z-First', 1); r.setHeader('A-Second', 2); r.setHeader('Z-FIRST', 3);
console.log(r.getHeaderNames().join(','), r.hasHeader('z-first'), r.hasHeader('Z-FIRST'));
console.log(r.removeHeader('z-FIRST') === undefined, r.getHeader('z-first') === undefined, r.hasHeader('z-first'));
r.setHeader('Z-First', 4); console.log(r.getHeaderNames().join(','));
