function snapshot(o: any) {
  const keys = [];
  for (const k in o) keys.push(k);
  console.log(Object.hasOwn(o, "later"), o.hasOwnProperty("later"), "later" in o,
    Object.keys(o).join(","), keys.join(","), JSON.stringify(o), JSON.stringify({...o}));
}
class LayoutSlots {
  constructor(probe: (o: any) => void) {
    probe(this);
    this.later = undefined;
    this.present = 7;
  }
}
const slots = new LayoutSlots(snapshot);
snapshot(slots);
delete (slots as any).later;
snapshot(slots);
(slots as any).later = 9;
snapshot(slots);
class LayoutBase { constructor() { this.base = 1; } }
class LayoutDerived extends LayoutBase { constructor(probe: (o: any) => void) {
  super(); probe(this); this.later = undefined;
} }
snapshot(new LayoutDerived(snapshot));

const spilled: any = {};
for (let i = 0; i < 12; i++) spilled["k" + i] = i === 11 ? undefined : i;
const copied = {...spilled};
console.log(Object.hasOwn(spilled, "k11"), Object.hasOwn(copied, "k11"),
  Object.keys(copied).join(","), JSON.stringify(copied));
