// Keep class evaluations and receivers alive across dense moving-GC schedules.
function factory(n: number) {
  return class {
    #n = n;
    #read() { return this.#n; }
    static has(o: any) { return #n in o && #read in o; }
    static read(o: any) { return o.#read(); }
    *values() { yield this.#n; }
  };
}
const classes: any[] = [], objects: any[] = [];
for (let i = 0; i < 160; i++) {
  const C = factory(i);
  const o: any = new C();
  classes.push(C);
  objects.push(o);
  // Keep the birth evaluation after its observable prototype is replaced.
  if (i % 3 === 0) Object.setPrototypeOf(o, null);
}
let sum = 0, own = 0, foreign = 0;
for (let pass = 0; pass < 3; pass++) {
  for (let i = 0; i < objects.length; i++) {
    const C: any = classes[i], o: any = objects[i];
    sum += C.read(o);
    own += C.has(o) ? 1 : 0;
    foreign += C.has(objects[(i + 1) % objects.length]) ? 1 : 0;
    const g = C.prototype.values.call(o);
    sum += g.next().value;
  }
}
console.log("brands", sum, own, foreign);
