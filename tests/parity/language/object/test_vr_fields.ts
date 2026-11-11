// VR-3a (DESIGN-p4l4 §5.1): a record binding is its fields by value. Its
// reads are its field locals, its assignments write them, and a boundary
// (console.log, JSON.stringify, a template literal, a boundary-only
// wrapper) takes a copy that prints as Node prints the object.
// vr-expect: fx,fy,fz ADMITTED
// vr-expect: qa,qb,qc ADMITTED
let total = 0;
for (let i = 0; i < 5; i++) {
  const p = { fx: i, fy: i * 2, fz: i % 2 === 0 };
  total += p.fz ? p.fx : p.fy;
}
console.log(total);
// One literal site assigned in a loop that reads the binding: every value
// is taken before a field is written.
let q: { qa: number; qb: number; qc: boolean } | undefined = undefined;
for (let i = 0; i < 4; i++) {
  q = { qa: q ? q.qb : 1, qb: q ? q.qa + i : 2, qc: q ? !q.qc : true };
}
if (q !== undefined) {
  console.log(q.qa, q.qb, q.qc);
  console.log(q);
  console.log("%o %j", q, q);
  console.log(`${q.qa}:${q.qb}`, JSON.stringify(q), [q, total], { q });
  const r = q;
  console.log(r.qa + r.qb, r);
}
