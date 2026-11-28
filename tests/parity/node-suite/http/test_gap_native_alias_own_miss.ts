// A read whose receiver's own list proves the key absent, on receivers of
// non-Ordinary record kinds that still answer it. The native-alias receiver
// (`http.Server.call(this)`) forwards such a read to its native server
// (`timeout`, `keepAliveTimeout`, `listen`): the probe's own miss must decline
// on its kind, not read the prototype chain, which answers undefined. The
// Array subclass `length`, the class subclass of http.Server and the
// typed-array prototypes are the neighbouring kinds. Generic named and
// computed sites, three rounds.
import * as http from "node:http";

const H: any = http;
function S(this: any) { H.Server.call(this, () => {}); }
Object.setPrototypeOf(S.prototype, H.Server.prototype);
class C extends H.Server {}
class A extends Array {}
const a = new A(); a.push(1, 2, 3);
const b = new A(); b.push(1.5, 2.5);
const recv: [string, any][] = [["s", new (S as any)()], ["c", new C()], ["a", a], ["b", b], ["u8p", Uint8Array.prototype],
  ["f64p", Float64Array.prototype], ["tap", Object.getPrototypeOf(Uint8Array.prototype)], ["o", { q: 1 }]];
const keys = ["length", "map", "subarray", "timeout", "keepAliveTimeout", "listen", "zz", "BYTES_PER_ELEMENT"];
function rmap(x: any) { return x.map; }
function rsub(x: any) { return x.subarray; }
function rtmo(x: any) { return x.timeout; }
function rkat(x: any) { return x.keepAliveTimeout; }
function rlis(x: any) { return x.listen; }
function rzz(x: any) { return x.zz; }
function rbpe(x: any) { return x.BYTES_PER_ELEMENT; }
function rk(x: any, k: string) { return x[k]; }
const t = (v: any) => v === null ? "null" : typeof v === "function" ? "fn" : typeof v === "object" ? "obj" : String(v);
for (let round = 0; round < 3; round++) {
  const out: string[] = [];
  for (const [n, x] of recv) {
    const named = [rmap, rsub, rtmo, rkat, rlis, rzz, rbpe].map((f) => t(f(x))).join("/");
    const computed = keys.filter((k) => !(k === "length" && (n === "u8p" || n === "f64p" || n === "tap")))
      .map((k) => t(rk(x, k))).join("/");
    out.push(n + " " + named + " | " + computed);
  }
  if (round === 2) console.log(out.join("\n"));
}
process.exit(0);
