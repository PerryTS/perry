// Class families with hidden class-capture slots and with a computed key
// that is a template literal over string constants (object model step 8).

function makeCounter(step: number, name: string) {
  class Counter {
    count = 0;
    label: string;
    constructor(start: number) {
      this.count = start;
      this.label = name + ":" + start;
    }
    bump(): number {
      this.count += step;
      return this.count;
    }
    describe(): string {
      return `${name} ${this.label} ${this.count}`;
    }
  }
  return Counter;
}

const C1 = makeCounter(2, "a");
const C2 = makeCounter(5, "b");
const x = new C1(1);
const y = new C2(10);
x.bump();
y.bump();
console.log(x.describe(), y.describe());
console.log(Object.keys(x).join(","), JSON.stringify(y));
console.log(Object.getOwnPropertyNames(x).join(","));
console.log(JSON.stringify({ ...x }));

let total = 0;
for (let i = 0; i < 500; i++) {
  const c = new C1(i);
  total += c.bump();
}
console.log(total);

// A class that captures an outer binding the constructor reassigns.
function makeTracker() {
  let calls = 0;
  class Tracker {
    id: number;
    constructor() {
      calls++;
      this.id = calls;
    }
    calls(): number {
      return calls;
    }
  }
  return Tracker;
}
const T = makeTracker();
const t1 = new T();
const t2 = new T();
console.log(t1.id, t2.id, t1.calls(), Object.keys(t2).join(","));

// Template-literal keys over string constants.
const version = "1.2";
const TypeId = `~demo/Fiber/${version}`;
const Other = "pre-" + version + "-post";
class Fiber {
  [TypeId]: number;
  [Other] = "o";
  id: number;
  constructor(id: number) {
    this[TypeId] = id * 2;
    this.id = id;
  }
}
const f = new Fiber(4);
console.log(Object.keys(f).join(","), (f as any)[TypeId], (f as any)["pre-1.2-post"], f.id);
console.log(JSON.stringify(f));
const fs: Fiber[] = [];
for (let i = 0; i < 300; i++) fs.push(new Fiber(i));
console.log(fs.reduce((s, q) => s + (q as any)[TypeId] + q.id, 0));
