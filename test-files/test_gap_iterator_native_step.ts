// Native stepping must preserve the public protocol and IteratorClose.
function* numbers() { try { yield 1; yield 2; yield 3; } finally { console.log("generator-finally"); } }
function make(kind: string): any {
  if (kind === "array") return [1, 2, 3].values();
  if (kind === "typed") return new Uint8Array([1,2,3]).values();
  if (kind === "map") return new Map([[1, 1], [2, 2], [3, 3]]).values();
  if (kind === "set") return new Set([1, 2, 3]).values();
  if (kind === "string") return "a😀b"[Symbol.iterator]();
  if (kind === "segments") return new Intl.Segmenter().segment("a😀b")[Symbol.iterator]();
  if (kind === "generator") return numbers();
  let n = 0;
  const user = Object.create(Object.getPrototypeOf(Object.getPrototypeOf([].values())));
  user[Symbol.iterator] = function() { return this; };
  user.next = function() { n++; return {value:n, done:n > 3}; };
  return user;
}
function value(x: any): string {
  if (typeof x === "object") return x.segment;
  return String(x);
}
for (const kind of ["typed","array", "map", "set", "string", "segments", "generator", "user"]) {
  let text = "";
  for (const x of make(kind)) text += value(x) + "|";
  console.log(kind, "walk", text);
  const manual = make(kind);
  const first = manual.next(); const second = manual.next();
  console.log(kind, "manual", first !== second, value(first.value), value(second.value));
  for (const level of [0, 1, 2]) {
    for (const method of ["next", "return"]) {
      const it: any = make(kind);
      let target: any = it;
      if (level >= 1) target = Object.getPrototypeOf(target);
      if (level >= 2) target = Object.getPrototypeOf(target);
      const old = Object.getOwnPropertyDescriptor(target, method);
      const original = it.next.bind(it);
      let calls = 0;
      Object.defineProperty(target, method, {
        configurable: true, writable: true,
        value: function() {
          calls++;
          if (method === "next") return original();
          return {value: undefined, done: true};
        }
      });
      let seen = "";
      for (const x of it) { seen += value(x); if (seen.length > 0) break; }
      console.log(kind, level, method, seen, calls);
      if (old) Object.defineProperty(target, method, old); else delete target[method];
    }
  }
  for (const exit of ["break", "throw", "return"]) {
    const it: any = make(kind); let closes = 0;
    it.return = function() { closes++; return {done: true}; };
    function consume() {
      for (const x of it) {
        if (exit === "return") return value(x);
        if (exit === "throw") throw "body-error";
        break;
      }
      return "end";
    }
    let result = "";
    try { result = consume(); } catch (e) { result = String(e); }
    console.log(kind, exit, result, closes);
  }
  const it: any = make(kind);
  let closes = 0;
  it.return = function() { closes++; return {done:true}; };
  const [a, b] = it;
  console.log(kind, "binding", value(a), value(b), closes);
  const assign: any = make(kind); let assignCloses = 0;
  assign.return = function() { assignCloses++; return {done:true}; };
  let c: any, d: any;
  [c, d] = assign;
  console.log(kind, "assignment", value(c), value(d), assignCloses);
}
let sum = 0;
for (const x of [1,2,3].values()) {
  for (const y of new Set([10,20]).values()) sum += x * y;
}
console.log("nested", sum);
// The generic path must get done before value and must preserve fresh results.
let trace = "";
const custom: any = {
  [Symbol.iterator]() { return this; },
  next() {
    trace += "n";
    return { get done() { trace += "d"; return trace.length > 6; },
             get value() { trace += "v"; return 1; } };
  }
};
let count = 0;
for (const x of custom) count += x;
console.log("access-order", count, trace);

// GetIterator captures next once. Shape invalidation must not call a new method.
for (const kind of ["typed","array", "map", "set", "string", "segments", "user"]) {
  const it: any = make(kind); let patched = 0; let seen = "";
  for (const x of it) {
    seen += value(x);
    it.next = function() { patched++; return {done:true}; };
  }
  console.log(kind, "next-mid-loop", seen, patched);
}
let gets = 0, pulls = 0;
const getterNext: any = {
  [Symbol.iterator]() { return this; },
  get next() {
    gets++;
    return function() { pulls++; return {value: pulls, done: pulls > 2}; };
  }
};
let getterSum = 0;
for (const x of getterNext) getterSum += x;
console.log("captured-getter", gets, pulls, getterSum);
const midDestruct: any = [1,2,3].values();
let destructPatched = 0;
const [mx, my = 0] = midDestruct;
console.log("destruct-complete", mx, my, destructPatched);
