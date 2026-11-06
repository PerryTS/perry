// #7634: `arr.push(f())` must push onto the array `arr.push` resolved to
// BEFORE the argument ran. ES2024 evaluates the MemberExpression to a
// Reference first, so an argument that rebinds `arr` cannot redirect the push.
//
// Only observable when the receiver's binding is reachable for writing while
// the argument is evaluated — a module global, a captured-and-mutated local,
// or a direct assignment inside the argument itself. Each of those is a case
// below; the plain-local case at the end pins that the historical order is
// kept where it is unobservable.

// --- module global, plain push ---------------------------------------------
let a: number[] = [1];
function f(): number {
  a = [9];
  return 2;
}
a.push(f());
console.log(JSON.stringify(a));

// --- module global, spread push --------------------------------------------
let b: number[] = [1];
function g(): number[] {
  b = [9];
  return [2, 3];
}
b.push(...g());
console.log(JSON.stringify(b));

// --- captured-and-mutated local --------------------------------------------
function capturedLocal(): string {
  let c: number[] = [1];
  const rebind = (): number => {
    c = [9];
    return 2;
  };
  c.push(rebind());
  return JSON.stringify(c);
}
console.log(capturedLocal());

// --- the argument assigns the binding directly ------------------------------
function selfAssigning(): string {
  let d: number[] = [1];
  d.push(((): number => 2)());
  d.push((d = [9], 3));
  return JSON.stringify(d);
}
console.log(selfAssigning());

// --- the push still lands, and the discarded array still gets it ------------
let e: number[] = [1];
let keep: number[] = [];
function swap(): number {
  keep = e;
  e = [9];
  return 2;
}
e.push(swap());
console.log(JSON.stringify(e), JSON.stringify(keep));

// --- the result of `push` is the NEW LENGTH of the array pushed onto --------
let h: number[] = [1, 2, 3];
function rebindH(): number {
  h = [];
  return 4;
}
console.log(h.push(rebindH()));
console.log(JSON.stringify(h));

// --- unobservable: a plain local nothing else can reach ---------------------
function plainLocal(): string {
  const p: number[] = [1];
  p.push(two());
  p.push(...[3, 4]);
  return JSON.stringify(p);
}
function two(): number {
  return 2;
}
console.log(plainLocal());
