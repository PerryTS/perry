// Gap test for #6908 — the remaining Array.prototype mutators on Proxy
// receivers (reverse / sort / splice / fill / copyWithin), plus the
// receiver-less prototype-thunk TypeError.
//
// A Proxy receiver reaching the generic mutator dispatch used to fall through
// every normalization (`as_real_array` rightly rejects handle-band ids;
// `run_object_mutator` only accepts plain objects) and silently return
// `undefined` — no trap fired, nothing mutated. `fill`/`copyWithin` were
// additionally noop-backed on Array.prototype. And a mutator thunk invoked as
// a plain value (`const f = arr.push; f(3)`) had no receiver and silently
// no-opped where spec step 1 `ToObject(this)` throws.
// Compared byte-for-byte against `node --experimental-strip-types`.

// ---- reverse: mutation, identity, trap order ----
{
  const t: any = [3, 1, 2];
  const p: any = new Proxy(t, {});
  const r = p.reverse();
  console.log("reverse:", t.join(","), r === p);
}
{
  const log: string[] = [];
  const t: any = [1, 2, 3, 4];
  const p: any = new Proxy(t, {
    get(o: any, k: any, rc: any) {
      if (typeof k === "string") log.push("g" + k);
      return Reflect.get(o, k, rc);
    },
    set(o: any, k: any, v: any, rc: any) {
      log.push("s" + k + "=" + v);
      return Reflect.set(o, k, v, rc);
    },
  });
  p.reverse();
  console.log("reverse-traps:", t.join(","), log.join("|"));
}
{
  const t: any = [1, , 3, 4];
  const p: any = new Proxy(t, {});
  p.reverse();
  console.log("reverse-hole:", t.join(","), 0 in t, 1 in t, 2 in t, 3 in t);
}

// ---- sort: default (string) order and comparator, identity ----
{
  const t: any = [10, 9, 1];
  const p: any = new Proxy(t, {});
  const r = p.sort();
  console.log("sort:", t.join(","), r === p);
}
{
  const t: any = [10, 2, 33, 4];
  const p: any = new Proxy(t, {});
  p.sort((a: number, b: number) => a - b);
  console.log("sort-cmp:", t.join(","));
}

// ---- splice: remove/insert, fresh real Array result, holes, traps ----
{
  const t: any = [1, 2, 3, 4, 5];
  const p: any = new Proxy(t, {});
  const removed = p.splice(1, 2, "a", "b", "c");
  console.log("splice:", t.join(","), t.length, Array.isArray(removed), removed.join(","));
}
{
  const t: any = [1, , 3];
  const p: any = new Proxy(t, {});
  const removed = p.splice(1);
  console.log("splice-1arg:", t.join(","), t.length, removed.length, 0 in removed, 1 in removed);
}
{
  const log: string[] = [];
  const t: any = [1, 2, 3];
  const p: any = new Proxy(t, {
    set(o: any, k: any, v: any, rc: any) {
      log.push("s" + k + "=" + v);
      return Reflect.set(o, k, v, rc);
    },
    deleteProperty(o: any, k: any) {
      log.push("d" + k);
      return Reflect.deleteProperty(o, k);
    },
  });
  const removed = p.splice(0, 1);
  console.log("splice-traps:", t.join(","), removed.join(","), log.join("|"));
}
{
  const t: any = [1, 2, 3];
  const p: any = new Proxy(t, { deleteProperty() { return false; } });
  try {
    p.splice(0, 1);
    console.log("splice-refused: NO-THROW");
  } catch (e: any) {
    console.log("splice-refused:", e instanceof TypeError, e.message);
  }
  console.log("splice-refused-after:", t.join(","), t.length);
}

// ---- fill / copyWithin: mutation through traps, identity ----
{
  const t: any = [1, 2, 3, 4];
  const p: any = new Proxy(t, {});
  const r = p.fill(7, 1, 3);
  console.log("fill:", t.join(","), r === p);
}
{
  const t: any = [1, 2, 3, 4, 5];
  const p: any = new Proxy(t, {});
  const r = p.copyWithin(0, 3);
  console.log("copyWithin:", t.join(","), r === p);
}

// ---- .call forms hit the same trap loops ----
{
  const t: any = [1, 2, 3];
  const p: any = new Proxy(t, {});
  Array.prototype.reverse.call(p);
  console.log("call-reverse:", t.join(","));
}
{
  const t: any = [1, 2, 3, 4];
  const p: any = new Proxy(t, {});
  const rm: any = Array.prototype.splice.call(p, 1, 2);
  console.log("call-splice:", rm.join(","), t.join(","));
}
{
  const t: any = [1, 2, 3];
  const p: any = new Proxy(t, {});
  Array.prototype.fill.call(p, 8, 1);
  console.log("call-fill:", t.join(","));
}
{
  const t: any = [1, 2, 3, 4];
  const p: any = new Proxy(t, {});
  Array.prototype.copyWithin.call(p, 0, 2);
  console.log("call-copyWithin:", t.join(","));
}

// ---- receiver-less prototype thunks throw the ToObject TypeError ----
{
  const t: any = [1, 2];
  const f: any = t["push"];
  console.log("thunk-typeof:", typeof f);
  try {
    f(3);
    console.log("bare-push: NO-THROW");
  } catch (e: any) {
    console.log("bare-push:", e instanceof TypeError, e.message);
  }
  console.log("bare-push-after:", t.join(","));
}
{
  const t: any = [1, 2];
  const p: any = new Proxy(t, {});
  const g: any = p.push;
  try {
    g(3);
    console.log("proxy-bare-push: NO-THROW");
  } catch (e: any) {
    console.log("proxy-bare-push:", e instanceof TypeError);
  }
  console.log("proxy-bare-push-after:", t.join(","));
}
{
  const t: any = [1];
  t.push(9);
  const f: any = t["pop"];
  try {
    f();
    console.log("stale-this: NO-THROW");
  } catch (e: any) {
    console.log("stale-this:", e instanceof TypeError);
  }
  console.log("stale-this-after:", t.join(","));
}

// ---- regression guards: dense arrays and array-like objects unaffected ----
{
  const t: number[] = [3, 1, 2];
  t.reverse();
  t.sort((a, b) => a - b);
  const rem = t.splice(1, 1);
  t.fill(0, 0, 1);
  t.copyWithin(2, 0, 1);
  console.log("dense:", t.join(","), rem.join(","));
}
{
  const o: any = { 0: "c", 1: "a", 2: "b", length: 3 };
  Array.prototype.reverse.call(o);
  console.log("objlike-reverse:", o[0], o[1], o[2]);
  const o2: any = { 0: 5, 1: 3, 2: 4, length: 3 };
  const rm: any = Array.prototype.splice.call(o2, 0, 2);
  console.log("objlike-splice:", rm.join(","), o2[0], o2.length);
}
