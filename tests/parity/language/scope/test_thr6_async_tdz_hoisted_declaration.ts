// A hoisted `let`/`const` stays its binding's declaration, in a sync body and
// in an async body the state machine splits. A function declaration is
// hoisted, so a binding it reads is forward-captured wherever the
// declaration sits; reading it before the declarator runs throws a
// ReferenceError (node), with or without an `await` before the read.
async function tdz1() { try { g(); return "no-throw"; } catch (e) { return (e as Error).constructor.name; } const buf = new Float32Array(4); function g() { return buf[0]; } }
async function tdz2() { await 0; try { g(); return "no-throw"; } catch (e) { return (e as Error).constructor.name; } const buf = new Float32Array(4); function g() { return buf[0]; } }
async function relet() { let b: any = new Float32Array(4); function w(k: any, v: any) { b[k] = v; return b[k]; } const r1 = w(1, 2.5); b = [0, 0, 0]; const r2 = w(1, "s"); return [r1, r2, Array.isArray(b), b.length].join(","); }
function s1() { try { g(); return "no-throw"; } catch (e) { return (e as Error).constructor.name; } const buf = new Float32Array(4); function g() { return buf[0]; } }
function s2() { try { g(); return "no-throw"; } catch (e) { return (e as Error).constructor.name; } let v = 1; function g() { return v; } }
function s3() { const r = g(); const k = 5; function g() { return 1; } return r + k; }
function s4() { let out = "x"; { try { h(); } catch (e) { out = (e as Error).constructor.name; } const q = 2; function h() { return q; } } return out; }
async function a1() { await 0; let x = 1; const f = () => x; x = 7; return f(); }
async function a2() { await 0; const arr = new Int32Array(3); for (let i = 0; i < 3; i++) arr[i] = i * 2; await 0; return arr.join(","); }
async function a3() { let b: any = new Float32Array(4); b[1] = 2.5; await 0; b = [0, 0, 0]; b[1] = "s"; return [b[1], Array.isArray(b), b.length].join(","); }
async function a4() { var v = 1; await 0; function g() { return v; } v = 3; return g(); }
async function a5() { try { g(); return "no-throw"; } catch (e) { return (e as Error).constructor.name; } await 0; const c = 1; function g() { return c; } }
async function a6() { await 0; let u; const f = () => u; const r1 = f(); u = 4; return [r1, f()].join(","); }
// A loop body's binding is per iteration. One that a closure or a hoisted
// function declaration captures before its declaration is born at the
// block's entry; with an `await` between that birth and the declaration it
// must still be the binding the earlier capture holds, one per iteration.
async function a7() { let out = ""; for (let i = 0; i < 2; i++) { await 0; const c = i * 10; function g() { return c; } out += g() + ";"; } return out; }
async function a7l() { let out = ""; for (let i = 0; i < 2; i++) { await 0; let c = i * 10; function g() { return c; } out += g() + ";"; } return out; }
async function a7arr() { const fns: any[] = []; for (let i = 0; i < 2; i++) { await 0; const c = i * 10; function g() { return c; } fns.push(g); } return fns.map((f) => f()).join(";"); }
async function a7arrow() { const fns: any[] = []; for (let i = 0; i < 2; i++) { const f = () => c; await 0; const c = i * 10; fns.push(f); } return fns.map((f) => f()).join(";"); }
async function a7of() { const fns: any[] = []; for (const x of [3, 4]) { await 0; const c = x * 2; function g() { return c + x; } fns.push(g); } return fns.map((f) => f()).join(";"); }
async function a7ofx() { const fns: any[] = []; for (const x of [3, 4]) { fns.push(() => x); await 0; fns.push(() => x * 10); } return fns.map((f) => f()).join(";"); }
async function a7pre() { let out = ""; const fns: any[] = []; for (let i = 0; i < 2; i++) { const c = i * 10; function g() { return c; } await 0; out += g() + ";"; fns.push(g); } return out + fns.map((f) => f()).join(";"); }
async function a7tdz() { let out = ""; for (let i = 0; i < 2; i++) { await 0; try { out += g(); } catch (e) { out += (e as Error).constructor.name + ";"; } const c = i * 10; function g() { return c; } out += g() + ";"; } return out; }
async function a7w() { const fns: any[] = []; for (let i = 0; i < 2; i++) { const f = () => { c += 1; return c; }; await 0; let c = i * 10; fns.push(f); } return fns.map((f) => f()).join(";") + "|" + fns.map((f) => f()).join(";"); }
// Every activation of an async closure has its own bindings: a `const`
// declared before an `await` and read after it belongs to that call, also
// when several calls are suspended at once.
const ac1 = async (n: number) => { const chunk = [n]; await 0; return n + ":" + chunk[0]; };
const ac2 = async (n: number) => { const chunk = [n, n]; const out = await Promise.resolve(await Promise.resolve(n * 2)); return n + ":" + chunk[1] + ":" + out; };
async function acs() {
  async function ac3(n: number) { let k = n + 1; const t = new Float32Array(1); t[0] = n; await 0; return n + ":" + k + ":" + t[0]; }
  const ac4 = async (n: number) => { try { g(); } catch (e) { return n + ":" + (e as Error).constructor.name; } await 0; const c = n; function g() { return c; } return "no-throw"; };
  const all = await Promise.all([ac1(1), ac1(2), ac2(3), ac2(4), ac3(5), ac3(6), ac4(7), ac4(8)]);
  return all.join(",");
}
async function run(n: string, f: () => Promise<string>) { try { console.log(n, await f()); } catch (e) { console.log(n, "threw", (e as Error).constructor.name); } }
async function main() {
  console.log("tdz1", await tdz1());
  console.log("tdz2", await tdz2());
  console.log("relet", await relet());
  console.log("s1", s1()); console.log("s2", s2()); console.log("s3", s3()); console.log("s4", s4());
  console.log("a1", await a1()); console.log("a2", await a2()); console.log("a3", await a3());
  console.log("a4", await a4()); console.log("a5", await a5()); console.log("a6", await a6());
  await run("a7", a7);
  await run("a7l", a7l);
  await run("a7arr", a7arr);
  await run("a7arrow", a7arrow);
  await run("a7of", a7of);
  await run("a7ofx", a7ofx);
  await run("a7pre", a7pre);
  await run("a7tdz", a7tdz);
  await run("a7w", a7w);
  await run("acs", acs);
}
main();
