// Test closures capturing different types simultaneously
// Critical for static typing migration: captured vars must maintain their native types

// === Closure capturing number, string, boolean ===
function makeGreeter(name: string, count: number, loud: boolean): () => string {
  return (): string => {
    const prefix = loud ? "HELLO" : "hello";
    return prefix + " " + name + " #" + count;
  };
}

const greet = makeGreeter("world", 42, true);
console.log(greet()); // HELLO world #42

const quiet = makeGreeter("perry", 1, false);
console.log(quiet()); // hello perry #1

// === Closure capturing mutable number ===
function makeCounter(): { inc: () => void, get: () => number } {
  let count = 0;
  return {
    inc: (): void => { count++; },
    get: (): number => count,
  };
}

const ctr = makeCounter();
ctr.inc();
ctr.inc();
ctr.inc();
console.log(ctr.get()); // 3

// === Closure capturing array ===
function makeAccumulator(): { add: (x: number) => void, sum: () => number, len: () => number } {
  const items: number[] = [];
  return {
    add: (x: number): void => { items.push(x); },
    sum: (): number => {
      let total = 0;
      for (let i = 0; i < items.length; i++) total += items[i];
      return total;
    },
    len: (): number => items.length,
  };
}

const acc = makeAccumulator();
acc.add(10);
acc.add(20);
acc.add(30);
console.log(acc.sum()); // 60
console.log(acc.len()); // 3

// === Closure capturing class instance ===
class Config {
  value: number;
  constructor(v: number) { this.value = v; }
  doubled(): number { return this.value * 2; }
}

function makeProcessor(cfg: Config): (x: number) => number {
  return (x: number): number => x + cfg.doubled();
}

const proc = makeProcessor(new Config(5));
console.log(proc(10)); // 20 (10 + 5*2)
console.log(proc(0));  // 10 (0 + 5*2)

// === Multiple closures sharing captured state ===
function makePair(): { setA: (v: string) => void, setB: (v: number) => void, show: () => string } {
  let a = "default";
  let b = 0;
  return {
    setA: (v: string): void => { a = v; },
    setB: (v: number): void => { b = v; },
    show: (): string => a + ":" + b,
  };
}

const pair = makePair();
console.log(pair.show()); // default:0
pair.setA("hello");
pair.setB(42);
console.log(pair.show()); // hello:42

// === Nested closures ===
function outer(x: number): () => () => number {
  return (): () => number => {
    return (): number => x * 2;
  };
}

const inner = outer(21)()();
console.log(inner); // 42

// === Closure in loop (let scoping) ===
const funcs: (() => number)[] = [];
for (let i = 0; i < 5; i++) {
  funcs.push((): number => i);
}
// Each closure should capture its own i
console.log(funcs[0]()); // 0
console.log(funcs[2]()); // 2
console.log(funcs[4]()); // 4
