// #10601: the "2-entry" shape from the issue -- does NOT reproduce the
// instanceof misclassification. Only class `A` is declared before the
// crafted value is used as an `instanceof` RHS, so its class id (1) is the
// only one ever registered; the crafted value's embedded id (5) is never
// registered, so `class_ref_id`'s `is_class_id_registered` check (#10592)
// correctly rejects it and the runtime falls through to the spec TypeError.
// Both Node and Perry (post-#10592) agree here.
const dv = new DataView(new ArrayBuffer(8));
dv.setUint32(0, 0x7ffe0000, false);
dv.setUint32(4, 5, false);
const crafted: any = dv.getFloat64(0, false);

class A { x = 1; }

const badRhs: [string, any][] = [
  ["{}", {}],
  ["craftedNaN", crafted],
];
for (const [name, R] of badRhs) {
  for (const [lname, v] of [["sso3", "uri"], ["newA", new A()], ["null", null]] as [string, any][]) {
    try {
      console.log(`${lname} instanceof ${name}:`, (v as any) instanceof R);
    } catch (e: any) {
      console.log(`${lname} instanceof ${name}: ${e.constructor.name}: ${e.message}`);
    }
  }
}
