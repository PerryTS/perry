// #11322: (1) a native-module-constructed value assigned to a binding declared
// WITHOUT an initializer (`let u; u = new URL(s)`, mongodb 7.0.0 HostAddress)
// must behave exactly like `const u = new URL(s)`. (The EventEmitter
// spelling is covered by hand: importing "events" makes the gap harness
// ext-route the test.) (2) a non-special scheme with
// an authority and no path has an EMPTY pathname and no trailing `/` in href.
import { URL, URLSearchParams } from "url";
import { TextEncoder, TextDecoder } from "util";
import { StringDecoder } from "string_decoder";
import * as nodeUrl from "url";

function lateUrl(s: string) { let u; u = new URL(s); return [u.hostname, u.port, u.href, u.pathname, u.protocol, u.search, u.host].join("|"); }
function lateUrlTry(s: string) { let u; try { u = new URL(s); } catch (e) { throw e; } return u.hostname + ":" + u.port; }
function lateUrlBranch(s: string, f: boolean) { let u; if (f) { u = new URL(s); } else { u = new URL("http://other:9/"); } return u.host; }
function lateUrlVar(s: string) { var u; u = new URL(s); return u.hostname; }
function lateUrlNs(s: string) { let u; u = new nodeUrl.URL(s); return u.hostname; }
function lateUrlTyped(s: string) { let u: URL; u = new URL(s); return u.hostname; }
function lateUrlAny(s: string) { let u: any; u = new URL(s); return u.hostname; }
function reassign(s: string) { let u = new URL("http://first/"); u = new URL(s); return u.hostname; }
function lateSP() { let p; p = new URLSearchParams("a=1&b=2"); return p.get("b") + "," + p.toString() + "," + p.has("a"); }
function lateTE() { let t; t = new TextEncoder(); return Array.from(t.encode("hi")).join(","); }
function lateTD() { let t; t = new TextDecoder(); return t.decode(new Uint8Array([104, 105])); }
function lateSD() { let d; d = new StringDecoder("utf8"); return d.write(Buffer.from("ok")); }
function lateMap() { let m; m = new Map<string, number>(); m.set("a", 1); return m.get("a") + ":" + m.size; }
function lateDate() { let d; d = new Date(0); return d.toISOString(); }
function lateRe() { let r; r = new RegExp("b+"); return String(r.test("abbc")) + r.source; }
function lateBuf() { let b; b = Buffer.from("xyz"); return b.toString("hex") + ":" + b.length; }
class Holder {
  u: URL | undefined;
  constructor(s: string) { let t; t = new URL(s); this.u = t; }
  host() { return this.u!.hostname; }
}
class Field {
  u: any;
  set(s: string) { this.u = new URL(s); }
}
let modLevel;
modLevel = new URL("https://mod.example:8443/p?q=1#h");
function readMod() { return modLevel.hostname + "|" + modLevel.port + "|" + modLevel.hash; }

console.log("lateUrl:", lateUrl("x://h:1"));
console.log("lateUrlSpecial:", lateUrl("https://Ex.COM:8443/a/b?c=d"));
console.log("lateUrlTry:", lateUrlTry("mongodb://db.local:27017"));
console.log("lateUrlBranch:", lateUrlBranch("http://a:1/", true), lateUrlBranch("http://a:1/", false));
console.log("lateUrlVar:", lateUrlVar("x://v:2"));
console.log("lateUrlNs:", lateUrlNs("x://ns:3"));
console.log("lateUrlTyped:", lateUrlTyped("x://typed:4"));
console.log("lateUrlAny:", lateUrlAny("x://any:5"));
console.log("reassign:", reassign("x://second:6"));
console.log("lateSP:", lateSP());
console.log("lateTE:", lateTE());
console.log("lateTD:", lateTD());
console.log("lateSD:", lateSD());
console.log("lateMap:", lateMap());
console.log("lateDate:", lateDate());
console.log("lateRe:", lateRe());
console.log("lateBuf:", lateBuf());
console.log("Holder:", new Holder("x://held:7").host());
const f = new Field(); f.set("x://field:8"); console.log("Field:", f.u.hostname, f.u.port);
console.log("modLevel:", readMod());

// --- URL serialization: special vs non-special schemes ---
const inputs = [
  "iLoveJS://127.0.0.1:1234", "foo://a:5", "foo://a", "foo://a/", "foo://a/b", "foo://a?q=1", "foo://a#h",
  "foo://u:p@a:5", "mongodb://db.local:27017", "redis://127.0.0.1", "x://h:1",
  "http://a", "http://a:8080", "https://Ex.com", "ws://a:1", "wss://a", "ftp://a:21",
  "http://a?q", "https://a#f", "file:///tmp/x", "mailto:a@b", "urn:isbn:1",
];
for (const s of inputs) {
  const u = new URL(s);
  console.log(JSON.stringify([s, u.href, u.pathname, u.host, u.hostname, u.port, u.search, u.hash, u.protocol, String(u)]));
}
const m = new URL("foo://a:5"); m.search = "?z=1"; console.log("set search:", m.href, JSON.stringify(m.pathname));
const n = new URL("foo://a:5"); n.hash = "k"; console.log("set hash:", n.href);
const r = new URL("foo://a:5"); r.pathname = "p"; console.log("set pathname:", r.href, r.pathname);
