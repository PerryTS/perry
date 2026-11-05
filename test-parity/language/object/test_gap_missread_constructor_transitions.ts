function Construct(this: any, value: number) {
  this.seed = value;
  this.added = value + 1;
}
const base: any = { value: 3, method(n: number) { return this.seed + n; } };
const middle: any = Object.create(base);
Object.setPrototypeOf(Construct.prototype, middle);
function make(value: number): any { return new (Construct as any)(value); }
function invoke(o: any) { return o.method(7); }
let sum = 0;
for (let i = 0; i < 1000; i++) { const o = make(i); sum += o.added + invoke(o); }
console.log("warm", sum);
let setters = 0;
Object.defineProperty(middle, "added", {
  set(v) { setters++; this.captured = v; },
  get() { return this.captured * 2; }, configurable: true,
});
const intercepted = make(10);
console.log("setter", intercepted.added, setters, Object.hasOwn(intercepted, "added"));
delete middle.added;
const afterDelete = make(20);
console.log("delete", afterDelete.added, Object.hasOwn(afterDelete, "added"));
Object.defineProperty(middle, "added", { value: 99, writable: false, configurable: true });
try { make(30); console.log("readonly", "allowed"); } catch (e) { console.log("readonly", e instanceof TypeError); }
delete middle.added;
base.method = function(n: number) { return this.seed * n; };
console.log("method replace", invoke(afterDelete));
middle.method = function(n: number) { return this.seed - n; };
console.log("method shadow", invoke(afterDelete));
delete middle.method;
console.log("method unshadow", invoke(afterDelete));
Object.defineProperty(middle, "method", { get() { return function(n: number) { return this.seed + n + 100; }; }, configurable: true });
console.log("method getter", invoke(afterDelete));
delete middle.method;
let gets = 0, writes = 0;
const proxy = new Proxy(base, {
  get(t, k, r) { gets++; return Reflect.get(t, k, r); },
  set(t, k, v, r) { writes++; return Reflect.set(t, k, v, r); },
});
Object.setPrototypeOf(middle, proxy);
const proxied = make(40);
console.log("proxy", proxied.added, invoke(proxied), gets, writes);
Object.setPrototypeOf(middle, null);
console.log("null", make(50).added);
const restHolder: any = { method(...values: number[]) { return this.seed + values.length; } };
Object.setPrototypeOf(middle, restHolder);
console.log("rest", invoke(afterDelete));
restHolder.method = function(n: number) { return this.seed + n; }.bind({ seed: 80 });
console.log("bound", invoke(afterDelete));
const captured = 90;
restHolder.method = function(n: number) { return captured + this.seed + n; };
console.log("capture", invoke(afterDelete));

const changing: any = { step() { this.afterCall = 7; return this.seed; } };
function Mutating(this: any, n: number) { this.seed = n; }
Object.setPrototypeOf(Mutating.prototype, changing);
function step(o: any) { return o.step(); }
let changedSum = 0;
for (let i = 0; i < 1000; i++) changedSum += step(new (Mutating as any)(i));
console.log("pre-call shape", changedSum);

// Accessor literals start as empty ordinary records. An unrelated own
// descriptor must not prevent caching subsequent data-property additions.
function record(n: number) {
  const r: any = { get cookies() { return this.payload + 1; } };
  r.rawPayload = n;
  r.payload = n;
  r.body = r.payload;
  r.json = function() { return this.body; };
  r.stream = function() { return this.rawPayload; };
  return r;
}
let recordSum = 0;
for (let i = 0; i < 2000; i++) {
  const r = record(i);
  recordSum += r.cookies + r.json() + r.stream();
}
const described = record(3);
delete described.payload;
Object.defineProperty(described, "payload", { get() { return 40; }, configurable: true });
console.log("descriptor record", recordSum, described.cookies, described.body);

// A base constructor sees one pre-shape per subclass, and each successor
// can name a different function body at the same assignment site.
function appendBody(o: any, body: any) { o.dispatch = body; }
function bodyA() { return 11; }
function bodyB() { return 22; }
let bodySum = 0;
for (let i = 0; i < 2000; i++) {
  const a: any = { a: i }, b: any = { b: i };
  appendBody(a, bodyA); appendBody(b, bodyB);
  bodySum += a.dispatch() + b.dispatch();
}
console.log("shape bodies", bodySum);
let sharedSum = 0;
for (let i = 0; i < 2000; i++) {
  const a: any = { shared: i }, b: any = { shared: i };
  appendBody(a, bodyA); appendBody(b, bodyB);
  sharedSum += a.dispatch() + b.dispatch();
}
console.log("shared pre-shape bodies", sharedSum);
