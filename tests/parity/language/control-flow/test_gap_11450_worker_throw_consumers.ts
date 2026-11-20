// #11450 follow-up: an unresolvable `new Worker(x)` throws, but every
// expression that consumes it (push, property store, Map.set, a field push,
// an indexed store from a nested closure) must still compile — and a caught
// throw must leave the receiver untouched.
class Pool {
  workers: any[] = [];
  grow(n: number) {
    if (n > 5) this.workers.push(new Worker("pool-" + n));
    return this.workers.length;
  }
}
console.log("field push", new Pool().grow(1));

function pushCaptured(n: any) {
  var ws: any[] = [];
  const size = () => ws.length;
  if (n > 5) for (var i = 0; i < n; ++i) ws.push(new Worker("w" + i));
  return size();
}
console.log("push captured", pushCaptured(1));

function pushLocal(n: any) {
  var ws: any[] = [];
  if (n > 5) ws.push(new Worker("w" + n));
  return ws.length;
}
console.log("push local", pushLocal(1));

function propertyStore(n: any) {
  var o: any = {};
  const keys = () => Object.keys(o).length;
  if (n > 5) o.worker = new Worker("w" + n);
  return keys();
}
console.log("property", propertyStore(1));

function mapSet(n: any) {
  var m = new Map<any, any>();
  if (n > 5) m.set(n, new Worker("w" + n));
  return m.size;
}
console.log("map", mapSet(1));

function nestedWriter(n: any) {
  var ws: any[] = [];
  function outer() {
    function inner() {
      if (n > 5) ws[ws.length] = new Worker("w" + n);
      return ws.length;
    }
    return inner();
  }
  return outer();
}
console.log("nested", nestedWriter(1));

function caughtPush(n: any) {
  const ws: any[] = [1];
  const size = () => ws.length;
  try {
    ws.push(new Worker("missing-" + n));
    console.log("unreachable push");
  } catch (_) {
    return "caught " + size();
  }
  return "no throw";
}
console.log("push", caughtPush(1));

function caughtProperty(n: any) {
  const o: any = { a: 1 };
  try {
    o.worker = new Worker("missing-" + n);
  } catch (_) {
    return "caught " + Object.keys(o).join(",");
  }
  return "no throw";
}
console.log("property", caughtProperty(1));

function live() {
  const ws: any[] = [];
  const size = () => ws.length;
  for (let i = 0; i < 4; i++) ws.push({ id: i });
  const o: any = {};
  o.last = ws[3];
  return size() + ":" + o.last.id;
}
console.log("live", live());
