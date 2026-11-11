// VR-3a (DESIGN-p4l4 §5.1): a record binding is its fields by value. Its
// reads are its field locals, its assignments write them, and a boundary
// (console.log, JSON.stringify, a template literal) takes a copy that
// prints as Node prints the object.
// vr-expect: fx,fy,fz ADMITTED
let total = 0;
for (let i = 0; i < 5; i++) {
  const p = { fx: i, fy: i * 2, fz: i % 2 === 0 };
  total += p.fz ? p.fx : p.fy;
}
console.log(total);
// A mutable binding assigned a literal that reads it: every value is taken
// before a field is written.
let q = { fx: 1, fy: 2, fz: true };
for (let i = 0; i < 4; i++) {
  q = { fx: q.fy, fy: q.fx + i, fz: !q.fz };
}
console.log(q.fx, q.fy, q.fz);
console.log(q);
console.log("%o %j", q, q);
console.log(`${q.fx}:${q.fy}`, JSON.stringify(q), [q, total], { q });
// One binding copied into another, and a conditional of two literals.
const r = q;
const c = Math.random() > 2;
const s = c ? { fx: 7, fy: 8, fz: false } : { fx: 9, fy: 10, fz: true };
console.log(r.fx + s.fx, r, s);
