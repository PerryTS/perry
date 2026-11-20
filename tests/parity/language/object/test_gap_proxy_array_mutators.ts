function invoke(method: string, proxy: any, borrowed: boolean): any {
  switch (method) {
    case "push": return borrowed ? Array.prototype.push.call(proxy, 7, 8) : proxy.push(7, 8);
    case "pop": return borrowed ? Array.prototype.pop.call(proxy) : proxy.pop();
    case "shift": return borrowed ? Array.prototype.shift.call(proxy) : proxy.shift();
    case "unshift": return borrowed ? Array.prototype.unshift.call(proxy, 7, 8) : proxy.unshift(7, 8);
    case "splice": return borrowed ? Array.prototype.splice.call(proxy, 1, 1, 7, 8) : proxy.splice(1, 1, 7, 8);
    case "sort": return borrowed ? Array.prototype.sort.call(proxy) : proxy.sort();
    case "reverse": return borrowed ? Array.prototype.reverse.call(proxy) : proxy.reverse();
  }
}
function invokeTyped(method: string, proxy: number[], borrowed: boolean): any {
  switch (method) {
    case "push": return borrowed ? Array.prototype.push.call(proxy, 7, 8) : proxy.push(7, 8);
    case "pop": return borrowed ? Array.prototype.pop.call(proxy) : proxy.pop();
    case "shift": return borrowed ? Array.prototype.shift.call(proxy) : proxy.shift();
    case "unshift": return borrowed ? Array.prototype.unshift.call(proxy, 7, 8) : proxy.unshift(7, 8);
    case "splice": return borrowed ? Array.prototype.splice.call(proxy, 1, 1, 7, 8) : proxy.splice(1, 1, 7, 8);
    case "sort": return borrowed ? Array.prototype.sort.call(proxy) : proxy.sort();
    case "reverse": return borrowed ? Array.prototype.reverse.call(proxy) : proxy.reverse();
  }
}
// Array builtins must observe the Proxy's original receiver and trap order.
function check(method: string, borrowed: boolean, sparse: boolean, typed: boolean) {
  const target: any[] = sparse ? [3, , 1] : [3, 1, 2];
  const events: string[] = [];
  const proxy = new Proxy(target, {
    get(t, k, r) { events.push("get:" + String(k)); return Reflect.get(t, k, r); },
    has(t, k) { events.push("has:" + String(k)); return Reflect.has(t, k); },
    set(t, k, v) { events.push("set:" + String(k) + "=" + String(v)); return Reflect.set(t, k, v); },
    deleteProperty(t, k) { events.push("delete:" + String(k)); return Reflect.deleteProperty(t, k); }
  });
  const result = typed ? invokeTyped(method, proxy, borrowed) : invoke(method, proxy, borrowed);
  console.log(method, borrowed, sparse, typed, events.join("|"));
  console.log(result === proxy ? "receiver" : JSON.stringify(result), JSON.stringify(target));
}
for (const method of ["push", "pop", "shift", "unshift", "splice", "sort", "reverse"]) {
  check(method, false, false, false);
  check(method, false, false, true);
  check(method, true, false, false);
  check(method, true, false, true);
  check(method, false, true, false);
  check(method, false, true, true);
  check(method, true, true, false);
  check(method, true, true, true);
}
// The get trap may replace any builtin; dispatch must call the returned value.
const names = ["push", "pop", "shift", "unshift", "splice", "sort", "reverse", "slice", "map"];
for (const name of names) {
  const p: any = new Proxy([1, 2, 3], {
    get(t, k, r) {
      if (k === name) return function() { return this === p ? "override" : "wrong-this"; };
      return Reflect.get(t, k, r);
    }
  });
  console.log("untyped", name, name === "slice" ? p.slice() : name === "map" ? p.map() : invoke(name, p, false));
  if (name !== "slice" && name !== "map") console.log("typed", name, invokeTyped(name, p, false));
}
// A draft can expose an array different from its physical Proxy target.
const backing = [1, 2, 3];
const draft: any = new Proxy([], {
  get(t, k, r) { return (backing as any)[k]; },
  has(t, k) { return Reflect.has(backing, k); },
  set(t, k, v) { return Reflect.set(backing, k, v); },
  deleteProperty(t, k) { return Reflect.deleteProperty(backing, k); }
});
console.log("virtual", draft.push(4), JSON.stringify(backing));
console.log("virtual-pop", draft.pop(), JSON.stringify(backing));
console.log("virtual-slice", JSON.stringify(Array.prototype.slice.call(draft)));

for (const method of ["push", "pop", "shift", "unshift", "splice", "sort", "reverse"]) {
  const events: string[] = [];
  const p = new Proxy([3, 1, 2], {
    get(t, k, r) { events.push("get:" + String(k)); return Reflect.get(t, k, r); },
    has(t, k) { events.push("has:" + String(k)); return Reflect.has(t, k); },
    set(t, k, v) { events.push("refuse-set:" + String(k)); return false; },
    deleteProperty(t, k) { events.push("refuse-delete:" + String(k)); return false; }
  });
  try { invokeTyped(method, p, false); console.log("refused", method, "missed"); }
  catch (e: any) { console.log("refused", method, e.name, events.join("|")); }
}

function spreadPush(a: number[], values: number[]) { return a.push(...values); }
function mixedPush(a: number[], values: number[]) { return a.push(7, ...values, 10); }
class Holder {
  items: number[];
  constructor(items: number[]) { this.items = items; }
  pushMany() { return this.items.push(7, 8); }
}
for (const mode of ["spread", "mixed", "field"]) {
  const events: string[] = [];
  const target = [1, 2, 3];
  const p = new Proxy(target, {
    get(t, k, r) { events.push("get:" + String(k)); return Reflect.get(t, k, r); },
    set(t, k, v) { events.push("set:" + String(k) + "=" + v); return Reflect.set(t, k, v); }
  });
  const result = mode === "spread" ? spreadPush(p, [7, 8]) :
    mode === "mixed" ? mixedPush(p, [8, 9]) : new Holder(p).pushMany();
  console.log(mode, result, JSON.stringify(target), events.join("|"));
}

// Dynamic index assignment must give the trap string keys, so records made
// by a draft can later be read with the same key (e.g. Immer's assigned_ Map).
function assignUnknown(a: any) { a[0] = 9; }
function assignField(holder: any) { holder.items[1] = 8; }
{
  const assigned = new Map<any, boolean>();
  const p = new Proxy([1, 2, 3], {
    set(t, k, v, r) {
      console.log("index-key", typeof k, String(k));
      assigned.set(k, true);
      return Reflect.set(t, k, v, r);
    }
  });
  assignUnknown(p);
  assignField({items: p});
  console.log("assigned", assigned.get("0"), assigned.get("1"), assigned.has(0), assigned.has(1));
}
