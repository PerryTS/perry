import * as http from "node:http";
const r: any = new http.ServerResponse({method:'GET'} as any);
const a = ['one']; r.setHeader('Z-First', a); r.setHeader('A-Second', 2);
const h = r.getHeaders(); console.log(Object.getPrototypeOf(h) === null, h['z-first'] === a, typeof h['a-second']);
h['z-first'].push('two'); h['a-second'] = 3; h.extra='x';
console.log(r.getHeader('z-first').join(','), r.getHeader('a-second'), r.hasHeader('extra'), h !== r.getHeaders());

const descriptor=Object.getOwnPropertyDescriptor(h,'a-second')!;
console.log(descriptor.value===3,descriptor.writable,descriptor.enumerable,descriptor.configurable);
delete h['z-first'];console.log(r.getHeader('z-first')===a);
