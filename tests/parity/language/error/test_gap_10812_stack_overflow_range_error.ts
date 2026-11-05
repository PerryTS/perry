// #10812: unbounded recursion throws a catchable
// `RangeError: Maximum call stack size exceeded` (it used to SIGSEGV).
function down(n: number): number {
  return down(n + 1) + 1;
}
try {
  down(0);
  console.log("unreachable");
} catch (e: any) {
  console.log("function:", e instanceof RangeError, e.message);
}

// Method recursion, and recovery: the program keeps running normally.
class Walker {
  walk(n: number): number {
    return this.walk(n + 1) + 1;
  }
}
try {
  new Walker().walk(0);
} catch (e: any) {
  console.log("method:", e instanceof RangeError, e.message);
}

// Closure recursion through an intermediate array callback.
const deep = (n: number): number => [n].map((x) => deep(x + 1))[0];
try {
  deep(0);
} catch (e: any) {
  console.log("closure:", e instanceof RangeError, e.message);
}

// Getter recursion without any explicit call expression.
class Loop {
  get a(): number {
    return (this as any).a;
  }
}
try {
  new Loop().a;
} catch (e: any) {
  console.log("getter:", e instanceof RangeError, e.message);
}

// Defensive depth handling: fall back to an iterative walk.
function sumRec(n: number): number {
  return n === 0 ? 0 : n + sumRec(n - 1);
}
function sumSafe(n: number): number {
  try {
    return sumRec(n);
  } catch (e) {
    let s = 0;
    for (let i = 1; i <= n; i++) s += i;
    return s;
  }
}
console.log("bounded:", sumSafe(1000), "fallback:", sumSafe(10_000_000));
