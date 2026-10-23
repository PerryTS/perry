// Reuse the same read/call sites across prototype mutations.
function next(it: any, value?: any) { return it.next(value); }
function* values() {
  try { yield 1; yield 2; }
  catch (error) { yield String(error); }
  return 3;
}
function show(label: string, value: any) {
  console.log(label, JSON.stringify(value));
}
const intrinsic = Object.getPrototypeOf(values.prototype);
const descriptor = Object.getOwnPropertyDescriptor(intrinsic, "next")!;
const it: any = values();
console.log("iterator", it[Symbol.iterator]() === it);
show("next", next(it));
show("throw", it.throw("caught"));
show("done", next(it));
show("return", values().return(9));
const returning = values();
show("before return", next(returning));
show("early return", returning.return(8));
for (const name of ["next", "return", "throw"]) {
  const d = Object.getOwnPropertyDescriptor(intrinsic, name)!;
  console.log(name, d.value.name, d.value.length, d.writable, d.enumerable, d.configurable);
}
const warmed: any = values();
show("warm", next(warmed));
intrinsic.next = function () { return { value: "intrinsic", done: false }; };
show("intrinsic replacement", next(warmed));
Object.defineProperty(intrinsic, "next", descriptor);
(values.prototype as any).next = function () { return { value: "function prototype", done: false }; };
show("function replacement", next(warmed));
delete (values.prototype as any).next;
show("restored", next(warmed));
delete intrinsic.next;
console.log("deleted", typeof warmed.next);
try { next(warmed); } catch (error) { console.log("deleted call", error instanceof TypeError); }
Object.defineProperty(intrinsic, "next", descriptor);
show("restored again", next(warmed));
const moved: any = values();
show("before setPrototypeOf", next(moved));
Object.setPrototypeOf(moved, {
  next() { return { value: "new prototype", done: true }; },
  [Symbol.iterator]() { return this; },
});
show("after setPrototypeOf", next(moved));
console.log("new iterator", moved[Symbol.iterator]() === moved);

async function* asyncValues() {
  try { yield 4; yield 5; }
  catch (error) { yield String(error); }
  return 6;
}
async function checkAsync() {
  const proto = Object.getPrototypeOf(asyncValues.prototype);
  const original = Object.getOwnPropertyDescriptor(proto, "next")!;
  const ai: any = asyncValues();
  console.log("async iterator", ai[Symbol.asyncIterator]() === ai);
  show("async next", await next(ai));
  show("async throw", await ai.throw("async caught"));
  show("async done", await next(ai));
  show("async return", await asyncValues().return(9));
  const aw: any = asyncValues();
  show("async warm", await next(aw));
  proto.next = async function () { return { value: "async intrinsic", done: false }; };
  show("async replacement", await next(aw));
  Object.defineProperty(proto, "next", original);
  (asyncValues.prototype as any).next = async function () {
    return { value: "async function prototype", done: false };
  };
  show("async function replacement", await next(aw));
  delete (asyncValues.prototype as any).next;
  show("async restored", await next(aw));
  delete proto.next;
  console.log("async deleted", typeof aw.next);
  try { await next(aw); } catch (error) { console.log("async deleted call", error instanceof TypeError); }
  Object.defineProperty(proto, "next", original);
  show("async restored again", await next(aw));
  const movedAsync: any = asyncValues();
  show("async before setPrototypeOf", await next(movedAsync));
  Object.setPrototypeOf(movedAsync, {
    async next() { return { value: "async new prototype", done: true }; },
    [Symbol.asyncIterator]() { return this; },
  });
  show("async after setPrototypeOf", await next(movedAsync));
  console.log("async new iterator", movedAsync[Symbol.asyncIterator]() === movedAsync);
}
checkAsync().catch(error => { console.log("unexpected", String(error)); });
