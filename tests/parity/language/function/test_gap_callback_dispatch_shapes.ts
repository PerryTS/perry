// Callback-dispatch shapes audited for perf/callback-dispatch (calling a
// JSValue that holds a closure through the various paths codegen/runtime
// recognize: direct arrow-inline array-method callback, a closure held in a
// local, a closure threaded through a second function frame, a
// `Function.prototype.bind` method reference with and without partial args,
// and a hand-written tight loop over a callback parameter).
//
// This exercises exactly the shape `dispatch_bound_function`
// (crates/perry-runtime/src/closure/dispatch/bound.rs) treats specially:
// `.bind(thisArg)` with NO extra bound args skips the old per-call `Vec`
// combine-copy and passes the call-time args straight through. What is easy
// to break doing that: `this` binding (arrow vs ordinary function vs bound),
// `arguments`, extra/missing arguments and `.length`, a callback that
// throws (stack must stay correct), recursion through a callback reference,
// and a callback closing over a loop variable.

function log(label: string, value: unknown): void {
  console.log(label + ": " + JSON.stringify(value));
}

// --- shape: direct arrow inline to a builtin array-iteration method -------
{
  const arr = [1, 2, 3, 4, 5];
  let s = 0;
  arr.forEach((x) => { s += x; });
  log("forEach_arrow_inline", s);
}

// --- shape: callback held in a local, called once -------------------------
{
  const cb = (x: number) => x + 1;
  log("local_once", cb(5));
}

// --- shape: callback held in a local, called in a tight loop --------------
{
  const cb = (x: number) => x * 2;
  let s = 0;
  for (let i = 0; i < 20; i++) s += cb(i);
  log("local_loop", s);
}

// --- shape: callback threaded through a second function frame -------------
function invokeOnce(cb: (x: number) => number, x: number): number {
  return cb(x);
}
function twoFrames(cb: (x: number) => number, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += invokeOnce(cb, i);
  return s;
}
log("two_frames", twoFrames((x) => x + 3, 15));

// --- shape: callback parameter called directly in a tight loop ------------
function directLoop(cb: (i: number) => number, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += cb(i);
  return s;
}
log("direct_loop_param", directLoop((x) => x - 1, 12));

// --- shape: method reference via .bind(), NO extra bound args -------------
class Adder {
  base: number;
  constructor(base: number) { this.base = base; }
  add(x: number): number { return this.base + x; }
  // reads `this` explicitly, so a wrong receiver after bind() is observable.
  describe(): string { return "Adder(" + this.base + ")"; }
}
{
  const a = new Adder(10);
  const fn = a.add.bind(a);
  const arr = [1, 2, 3, 4, 5];
  let s = 0;
  arr.forEach((x) => { s += fn(x); });
  log("bound_no_extra_args_forEach", s);
  log("bound_no_extra_args_once", fn(7));

  const describeFn = a.describe.bind(a);
  log("bound_this_reads_correctly", describeFn());
}

// --- shape: method reference via .bind(), WITH partial-applied args -------
{
  const a = new Adder(100);
  const fn2 = a.add.bind(a, 5); // bound arg `5` prepended... but `add` takes
  // only ONE param, so the extra bound arg is simply ignored per spec (bound
  // length caps at 0, extra bound args beyond declared arity are dropped by
  // the underlying call, not by bind itself -- Node and Perry must agree).
  log("bound_with_extra_args", fn2(1));

  function sum3(a: number, b: number, c: number): number { return a + b + c; }
  const boundSum = sum3.bind(null, 1, 2);
  log("bound_plain_fn_partial", boundSum(3));
}

// --- shape: this binding across arrow / ordinary function / bound ---------
{
  const obj = {
    v: 42,
    arrowGet(this: any) { return (() => this.v)(); },
    ordinary(this: any) { return this.v; },
  };
  function grabThis(this: any): unknown { return this; }
  const boundGrab = grabThis.bind(obj);
  log("this_arrow_capture", obj.arrowGet());
  log("this_ordinary_direct", obj.ordinary());
  log("this_bound_grab", (boundGrab() as { v: number }).v);

  // A receiverless call of an ordinary function callback observes
  // `this === undefined` (strict-mode-like OrdinaryCallBindThis) even when
  // an enclosing method call left an IMPLICIT_THIS around.
  function receiverless(this: unknown): string {
    return this === undefined ? "undefined" : "leaked:" + JSON.stringify(this);
  }
  function callIt(cb: () => string): string { return cb(); }
  const holder = {
    m(): string { return callIt(receiverless); },
  };
  log("this_receiverless_no_leak", holder.m());
}

// --- shape: arguments object + extra/missing args + .length ---------------
{
  function variadic(): string {
    // eslint-disable-next-line prefer-rest-params
    const args = arguments as unknown as ArgumentsLike;
    const parts: string[] = [];
    for (let i = 0; i < args.length; i++) parts.push(String(args[i]));
    return parts.join(",");
  }
  interface ArgumentsLike { length: number; [i: number]: unknown; }
  function callWithN(cb: (...a: unknown[]) => string, ...a: unknown[]): string {
    return cb(...a);
  }
  log("arguments_object_extra", callWithN(variadic as any, 1, 2, 3, 4));
  log("arguments_object_missing", callWithN(variadic as any));
  log("function_length_declared", ((a: number, b: number, c: number) => a + b + c).length);

  function needsThree(a: number, b: number, c: number): string {
    return `${a},${b},${c}`;
  }
  log("missing_args_become_undefined", (needsThree as any)(1));
  log("extra_args_ignored", (needsThree as any)(1, 2, 3, 4, 5));
}

// --- shape: callback that throws, stack must stay correct -----------------
{
  function boom(): never { throw new Error("boom"); }
  function callThrow(cb: () => never): string {
    try {
      cb();
      return "no-throw";
    } catch (e) {
      return "caught:" + (e as Error).message;
    }
  }
  log("callback_throws_caught", callThrow(boom));

  const boundBoom = boom.bind(null);
  let threwFromBound = "no";
  try {
    boundBoom();
  } catch (e) {
    threwFromBound = "caught:" + (e as Error).message;
  }
  log("bound_callback_throws", threwFromBound);

  function outer(): string {
    function inner(cb: () => never): string {
      try {
        cb();
        return "unreachable";
      } catch (e) {
        return (e as Error).message;
      }
    }
    return inner(boom);
  }
  log("callback_throws_through_two_frames", outer());
}

// --- shape: recursion through a callback reference -------------------------
{
  function makeCountdown(): (n: number) => number {
    const step = (n: number): number => (n <= 0 ? 0 : n + step(n - 1));
    return step;
  }
  const countdown = makeCountdown();
  log("recursive_callback", countdown(10));

  // Recursion through a bound reference to itself.
  let fact: (n: number) => number;
  fact = (n: number): number => (n <= 1 ? 1 : n * fact(n - 1));
  const boundFact = fact.bind(null);
  log("recursive_bound_callback", boundFact(6));
}

// --- shape: closure captured in a loop variable ----------------------------
{
  const callbacks: Array<() => number> = [];
  for (let i = 0; i < 5; i++) {
    callbacks.push(() => i * i);
  }
  log("loop_var_capture_let", callbacks.map((f) => f()));

  const callbacksVar: Array<() => number> = [];
  for (var j = 0; j < 5; j++) {
    // eslint-disable-next-line no-loop-func
    callbacksVar.push((function (captured) { return () => captured; })(j));
  }
  log("loop_var_capture_var_iife", callbacksVar.map((f) => f()));
}

// --- shape: bound method used as a hot forEach callback across many calls -
{
  class Acc {
    total: number = 0;
    add(x: number): number { this.total += x; return this.total; }
  }
  const acc = new Acc();
  const boundAdd = acc.add.bind(acc);
  const many: number[] = [];
  for (let i = 0; i < 200; i++) many.push(i);
  many.forEach((x) => boundAdd(x));
  log("bound_hot_loop_total", acc.total);
}
