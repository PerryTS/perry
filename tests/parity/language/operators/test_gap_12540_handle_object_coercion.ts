// #12540: native handle ids (Headers, Blob, Response, URL, streams, timers)
// carry their own NaN-box tag (HANDLE_TAG) since #12529, but in JS terms each
// one IS an object. Every coercion must run ToPrimitive on it like on any
// other object: `1 / headers` is NaN of type number, `headers + 1` is
// "[object Headers]1". Byte-identical to node --experimental-strip-types.

function inv(x: number): number { return 1 / x; }
function sub(a: number, b: number): number { return a - b; }
function mul(a: number, b: number): number { return a * b; }
function add(a: any, b: any): any { return a + b; }
function pos(a: any): any { return +a; }
function neg(a: any): any { return -a; }
function inc(a: any): any { a++; return a; }
function lt(a: any, b: any): boolean { return a < b; }
function ge(a: any, b: any): boolean { return a >= b; }
function eq(a: any, b: any): boolean { return a == b; }
function seq(a: any, b: any): boolean { return a === b; }
function tmpl(a: any): string { return `<${a}>`; }

const timer: any = setTimeout(() => {}, 100000);
const kinds: Array<[string, any]> = [
  ["Headers", new Headers({ a: "1" })],
  ["Blob", new Blob(["xy"])],
  ["Response", new Response("r")],
  ["Request", new Request("http://example.com/")],
  ["URL", new URL("http://example.com/p?q=1")],
  ["ReadableStream", new ReadableStream()],
  ["AbortController", new AbortController()],
  ["Timeout", timer],
  // Negative control: a plain heap object takes the same paths.
  ["plain", { k: 1 }],
];

for (const [name, h] of kinds) {
  const anyInv: any = inv;
  const r = anyInv(h);
  const isTimer = name === "Timeout";
  console.log(name, "typeof", typeof h, "| 1/x", typeof r, isTimer ? "" : String(r));
  // A Timeout's ToPrimitive is its numeric id, which differs between engines.
  if (!isTimer) {
    console.log(name, "x-2", String((sub as any)(h, 2)), "| 3-x", String((sub as any)(3, h)));
    console.log(name, "x*2", String((mul as any)(h, 2)), "| +x", String(pos(h)), "| -x", String(neg(h)));
    console.log(name, "x++", String(inc(h)));
    console.log(name, "x+1", add(h, 1), "| 's'+x", add("s", h));
    console.log(name, "tmpl", tmpl(h), "| String", String(h), "| Number", Number(h));
    console.log(name, "x<1", lt(h, 1), "| x>=0", ge(h, 0), "| x<'z'", lt(h, "z"));
    console.log(name, "x=='[object]'", eq(h, String(h)));
  } else {
    console.log(name, "+x is number", typeof pos(h), "| x+'' is string", typeof add(h, ""));
  }
  console.log(name, "x==x", eq(h, h), "| x===x", seq(h, h), "| x==null", eq(h, null), "| x===undefined", seq(h, undefined));
  console.log(name, "x==0", eq(h, 0) && !isTimer, "| x==={}", seq(h, {}));
  console.log(name, "truthy", !!h, "| not", !h, "| ternary", h ? "yes" : "no", "| and", (h && 1) === 1);
}
clearTimeout(timer);
