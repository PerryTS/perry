// for-of over generator objects the whole program proves: equal to node.
function* range(a: number, b: number): Generator<number> { for (let i = a; i < b; i++) yield i; }
let s = 0;
for (const v of range(0, 10)) s += v;
console.log("plain", s);
function* twice(it: Generator<number>): Generator<number> { for (const v of it) yield v * 2; }
s = 0;
for (const v of twice(range(0, 5))) s += v;
console.log("nested", s);
const log: string[] = [];
function* fin(): Generator<number> { try { yield 1; yield 2; yield 3; } finally { log.push("finally"); } }
for (const v of fin()) { log.push("v" + v); if (v === 2) break; }
console.log("break", log.join(","));
function* thrower(): Generator<number> { yield 1; yield 2; throw new Error("boom"); }
const t = thrower();
const seen: number[] = [];
try { for (const v of t) seen.push(v); } catch (e) { console.log("caught", (e as Error).message, seen.join(",")); }
const after = t.next();
console.log("after-throw", after.value, after.done);
let self: Generator<number>;
function* reent(): Generator<number> { yield 1; try { for (const v of self) console.log("never", v); } catch (e) { console.log("reentry", e instanceof TypeError); } yield 2; }
self = reent();
const rs: number[] = [];
for (const v of self) rs.push(v);
console.log("reent", rs.join(","));
function* inner(): Generator<number> { yield 1; yield 2; return 9; }
function* outer(): Generator<number> { const r = yield* inner(); yield r; yield 3; }
const os: number[] = [];
for (const v of outer()) os.push(v);
console.log("delegate", os.join(","));
const part = range(0, 6);
const ps: number[] = [];
for (const v of part) { ps.push(v); if (v === 2) break; }
console.log("partial", ps.join(","), JSON.stringify(part.next()));
function* body2(): Generator<number> { let x = yield 1; while (true) { x = yield x; } }
const b2 = body2();
const bs: number[] = [];
for (const v of b2) { bs.push(v); if (bs.length > 3) break; }
console.log("sent", bs.join(","), JSON.stringify(b2.next(5)));
function* retEarly(): Generator<number> { try { yield 1; yield 2; } finally { console.log("ret-finally"); } }
function firstOf(): number { for (const v of retEarly()) return v; return -1; }
console.log("return", firstOf());
function* throwOut(): Generator<number> { try { yield 1; } finally { console.log("throw-finally"); } }
try { for (const v of throwOut()) throw new Error("out" + v); } catch (e) { console.log("thrown", (e as Error).message); }
