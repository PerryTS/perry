'use strict';
function add(a: any[], x: any) { a[a.length] = x; }
// A large old backing receives young children after a mixed scalar prefix.
// Keep aliases to both the forwarding head and the source string.
const live: any = [];
const alias: any = live;
for (let i = 0; i < 4096; i++) add(live, i);
for (let i = 0; i < 12000; i++) add(live, { n: i, s: 'append value ' + i });
let sum = 0;
for (let r = 0; r < 48; r++) {
    const scratch: any = [];
    for (let i = 0; i < 4000; i++) {
        add(scratch, { n: i, s: 'young value ' + i });
        if (i % 7 === 0) add(scratch, i);
    }
    sum += scratch[0].n + scratch[0].s.length;
}
for (let i = 0; i < 12000; i++) {
    const item = alias[4096 + i];
    sum += item.n + item.s.length;
}
console.log(alias.length, alias[4095], sum);
