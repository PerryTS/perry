// #6759/#6809 acceptance: property writes to existing fields.
const objs: any[] = [];
for (let i = 0; i < 2400; i++) {
  objs.push({ a: i, b: i * 2, c: 0, d: 0 });
}

const t0 = Date.now();
for (let r = 0; r < 2000; r++) {
  for (let i = 0; i < 2400; i++) {
    const o = objs[i];
    o.c = r + i;
    o.d = r - i;
  }
}
const t1 = Date.now();

let sink = 0;
for (let i = 0; i < 2400; i++) {
  sink += objs[i].c + objs[i].d;
}
console.log("write_ms", t1 - t0, "sink", sink);
