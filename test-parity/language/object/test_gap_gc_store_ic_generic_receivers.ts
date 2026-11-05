// Receivers whose store must NEVER take the static-key store IC's inline
// path, fed through sites that were first primed on plain receivers of the
// same key. A frozen / sealed-but-writable / non-writable / accessor receiver
// is a different ShapeId (integrity and descriptor installs re-stamp it); a
// Proxy is a handle-band id the receiver test refuses; a URL's own fields are
// live views the runtime must route through its setter; an inherited setter
// is not an own data slot at all. Each must behave exactly as node.

function put(o: any, v: any): void {
  o.k = v;
}
function putPath(o: any, v: any): void {
  o.pathname = v;
}

const out: string[] = [];
function show(label: string, o: any): void {
  let k: any;
  try {
    k = o.k;
  } catch (e) {
    k = "read-threw";
  }
  out.push(label + " k=" + String(k));
}
function attempt(label: string, o: any, v: any): void {
  try {
    put(o, v);
    put(o, v + 1); // a second store: a primed-on-this-receiver site would hit here
  } catch (e) {
    out.push(label + " threw " + (e as Error).constructor.name);
  }
  show(label, o);
}

for (let i = 0; i < 16; i++) {
  const p = { k: 0, z: i };
  put(p, i);
  put(p, i + 100);
}

const frozen = Object.freeze({ k: 1, z: 0 });
attempt("frozen", frozen, 50);

const sealed = Object.seal({ k: 1, z: 0 });
attempt("sealed", sealed, 60);

const nonWritable = { k: 1, z: 0 };
Object.defineProperty(nonWritable, "k", { writable: false });
attempt("non-writable", nonWritable, 70);

const accessor: any = { k: 1, z: 0 };
let seen = 0;
Object.defineProperty(accessor, "k", {
  get() {
    return "getter:" + seen;
  },
  set(v: number) {
    seen = v;
  },
});
attempt("accessor", accessor, 80);

class WithSetter {
  z = 0;
  store = 0;
  set k(v: number) {
    this.store = v * 10;
  }
  get k(): number {
    return this.store;
  }
}
const inherited = new WithSetter();
attempt("inherited-setter", inherited, 90);

const target = { k: 1, z: 0 };
const traps: string[] = [];
const proxy = new Proxy(target, {
  set(t: any, key: string | symbol, v: any) {
    traps.push("set " + String(key) + "=" + v);
    t[key] = v * 2;
    return true;
  },
});
attempt("proxy", proxy, 100);
out.push("traps " + traps.join(","));
out.push("proxy target k=" + target.k);

for (let i = 0; i < 16; i++) {
  const p = { pathname: "/p" + i, z: i };
  putPath(p, "/q" + i);
}
const url = new URL("https://example.com/start?x=1");
putPath(url, "/one");
putPath(url, "/two");
out.push("url " + url.href + " " + url.pathname);

// A URL through a site primed on a plain object carrying URL's field names:
// a URL's own `pathname` slot is a live view whose store must go through the
// setter that rebuilds `href` — a URL is never marked ordinary, so neither
// the publication nor the hit's receiver-kind test may admit it.
function putPath2(o: any, v: any): void {
  o.pathname = v;
}
const twin = JSON.parse(
  '{"href":"h","protocol":"p","host":"h","hostname":"h","port":"","pathname":"/t",' +
    '"search":"","hash":"","origin":"o","searchParams":null,"username":"","password":""}',
);
for (let i = 0; i < 4; i++) putPath2(twin, "/t" + i);
const url2 = new URL("https://example.com/start?x=1");
putPath2(url2, "/three");
putPath2(url2, "/four");
out.push("url twin " + url2.href + " " + url2.pathname + " " + twin.pathname);

for (const line of out) console.log(line);
