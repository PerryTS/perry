// A `const` declared after a `yield` whose initializer holds an object-literal
// method declaring a same-named local (effect's SqlClient.make:
// `const client = Object.assign(make(), { withoutTransforms() { const client
// = ...; return () => client } }); client.safe = client`). HIR boxes the outer
// binding (the forward-capture scan is name-based) and preallocates the box at
// the body entry, which the generator split into an earlier resume state than
// the declaration; the declaring state then read `undefined`. opencode's
// `models` died with "Cannot set properties of undefined (setting 'safe')".

function mk(): any {
  return function sql() {
    return 1;
  };
}

function* sqlClient(opts: any): Generator<any, any, any> {
  const r = yield "acquire";
  const client: any = Object.assign(mk(), {
    safe: undefined,
    reserve: r,
    withoutTransforms(this: any) {
      if (opts.transformRows === undefined) return this;
      const client: any = Object.assign(mk(), { ...this });
      client.safe = client;
      client.withoutTransforms = () => client;
      return client;
    },
  });
  client.safe = client;
  return client;
}
const g = sqlClient({});
console.log("first", g.next().value);
const res = g.next("conn");
console.log("client", typeof res.value, res.value.safe === res.value, res.value.reserve, res.done);
console.log("without", res.value.withoutTransforms() === res.value);

function* plainMethod(): Generator<number, string, unknown> {
  yield 1;
  const o: any = { m() { const o = "inner"; return o; } };
  return typeof o + ":" + o.m();
}
console.log("plain", [...plainMethod()].length, plainMethod().next().value);
const pm = plainMethod();
pm.next();
console.log("plain result", pm.next().value);

function* nestedBlock(flag: boolean): Generator<number, string, unknown> {
  if (flag) {
    yield 1;
    const v: any = { get() { const v = 2; return v; } };
    return "nested " + typeof v + " " + v.get();
  }
  return "none";
}
const nb = nestedBlock(true);
nb.next();
console.log(nb.next().value);

function* severalStates(): Generator<number, string, unknown> {
  yield 1;
  const a: any = { f() { const a = 10; return a; } };
  yield 2;
  const b: any = { f() { const b = 20; return b; } };
  return [typeof a, a.f(), typeof b, b.f()].join(" ");
}
console.log([...severalStates()].join(","));
const ss = severalStates();
ss.next();
ss.next();
console.log("several", ss.next().value);

async function* asyncGen(): AsyncGenerator<number, string, unknown> {
  yield 1;
  const c: any = { m() { const c = 3; return c; } };
  return "async " + typeof c + " " + c.m();
}
(async () => {
  const ag = asyncGen();
  await ag.next();
  console.log((await ag.next()).value);
})();
