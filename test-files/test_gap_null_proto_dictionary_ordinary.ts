// Runtime-born null-prototype dictionaries (querystring.parse, Object.groupBy,
// node:sqlite rows, util.parseEnv) are born as the ordinary kind with a null
// prototype, the same birth as Object.create(null); the store lanes and plans
// trust that shape. Each result must still behave as a null-prototype data
// object, including after later relinks, freezes and descriptor changes.
import * as querystring from "node:querystring";
import * as util from "node:util";
import { DatabaseSync } from "node:sqlite";

function show(label: string, value: unknown): void {
  console.log(label, JSON.stringify(value));
}

function attempt(label: string, f: () => unknown): void {
  try {
    show(label, f());
  } catch (e) {
    console.log(label, "threw", (e as Error).constructor.name);
  }
}

function probe(label: string, make: () => any, key: string): void {
  const o = make();
  show(label + " proto", Object.getPrototypeOf(o) === null);
  show(label + " keys", Object.keys(o));
  show(label + " in", [key in o, "toString" in o, "hasOwnProperty" in o, "__proto__" in o]);
  show(label + " json", o);
  attempt(label + " String", () => String(o));
  show(label + " tag", Object.prototype.toString.call(o));
  const forIn: string[] = [];
  for (const k in o) forIn.push(k);
  show(label + " forin", forIn);

  // Named and keyed stores, adds and overwrites, `__proto__` as a key.
  const a = make();
  a.added = 1;
  a[key] = "over";
  const dyn = "dyn" + label.length;
  a[dyn] = 2;
  a.__proto__ = 7;
  a["toString"] = 8;
  show(label + " stores", [Object.keys(a), a.added, a[key], a[dyn], a.__proto__, a.toString]);
  show(label + " stores proto", Object.getPrototypeOf(a) === null);

  // Repeat the same store sequence on fresh siblings: plans and edges learned
  // on the first sibling serve the later ones.
  for (let i = 0; i < 3; i++) {
    const s = make();
    s.added = i;
    s[dyn] = i * 2;
    s.__proto__ = i;
    show(label + " sib" + i, [Object.keys(s), s.added, s[dyn], s.__proto__, Object.getPrototypeOf(s) === null]);
  }

  // A later relink: the new chain's accessor and readonly key now intercept.
  const r = make();
  const log: unknown[] = [];
  const proto = {
    set acc(v: unknown) {
      log.push(v);
    },
  };
  Object.defineProperty(proto, "ro", { value: 1, writable: false });
  Object.setPrototypeOf(r, proto);
  r.acc = 5;
  attempt(label + " relink ro", () => {
    r.ro = 2;
    return r.ro;
  });
  r.fresh = 3;
  show(label + " relink", [log, Object.keys(r), r.fresh, "acc" in r, Object.hasOwn(r, "acc")]);
  attempt(label + " relink String", () => typeof String(Object.setPrototypeOf(make(), Object.prototype)));
  Object.setPrototypeOf(r, null);
  r.acc = 6;
  show(label + " unlink", [log, Object.keys(r), r.acc]);

  // Own accessor and readonly added after birth.
  const d = make();
  const seen: unknown[] = [];
  Object.defineProperty(d, "own", {
    set(v) {
      seen.push(v);
    },
    get() {
      return "g";
    },
    configurable: true,
  });
  Object.defineProperty(d, "fixed", { value: 1, writable: false, enumerable: true });
  d.own = 1;
  attempt(label + " own ro", () => {
    d.fixed = 2;
    return d.fixed;
  });
  d.later = 4;
  show(label + " own", [seen, d.own, d.fixed, d.later, Object.keys(d)]);

  // Integrity levels.
  const f = Object.freeze(make());
  attempt(label + " frozen add", () => {
    f.x = 1;
    return f.x;
  });
  attempt(label + " frozen set", () => {
    f[key] = 1;
    return f[key];
  });
  const se = Object.seal(make());
  attempt(label + " sealed add", () => {
    se.x = 1;
    return se.x;
  });
  attempt(label + " sealed set", () => {
    se[key] = "sealed-set";
    return [se[key], Object.isSealed(se)];
  });
  const ne = Object.preventExtensions(make());
  attempt(label + " noext add", () => {
    ne.x = 1;
    return ne.x;
  });

  // Copies.
  show(label + " spread", { ...make() });
  show(label + " assign", Object.assign(Object.create(null), make()));
  const del = make();
  delete del[key];
  del[key] = "back";
  show(label + " readd", [Object.keys(del), del[key]]);
}

probe("qs", () => querystring.parse("a=1&b=2&__proto__=p&constructor=c"), "a");
probe("qs-empty", () => querystring.parse(""), "a");
probe(
  "groupBy",
  () => Object.groupBy([1, 2, 3, 4], (n: number) => (n % 2 ? "odd" : "even")),
  "odd",
);
probe("parseEnv", () => util.parseEnv("A=1\nB=two\n"), "A");

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE t (id INTEGER, name TEXT)");
db.exec("INSERT INTO t VALUES (1, 'x'), (2, 'y')");
const stmt = db.prepare("SELECT id, name FROM t ORDER BY id");
probe("sqlite get", () => stmt.get(), "name");
probe("sqlite all", () => stmt.all()[1], "name");
probe("sqlite iterate", () => stmt.iterate().next().value, "id");
probe("sqlite columns", () => stmt.columns()[0], "name");

// A polluted Object.prototype must not reach a null-prototype result.
(Object.prototype as any).toJSON = function () {
  return "polluted";
};
(Object.prototype as any).inherited = 1;
show("polluted qs", [querystring.parse("a=1"), "inherited" in querystring.parse("a=1")]);
show("polluted groupBy", Object.groupBy([1], () => "k"));
show("polluted sqlite", stmt.get());
show("polluted create", Object.create(null));
delete (Object.prototype as any).toJSON;
delete (Object.prototype as any).inherited;

// The store lanes on Object.create(null) receivers: a warm loop of named and
// keyed adds, then an accessor and a readonly key added later to a sibling.
const keys = ["k0", "k1", "k2", "k3"];
const bags: any[] = [];
for (let i = 0; i < 50; i++) {
  const b = Object.create(null);
  b.first = i;
  for (const k of keys) b[k] = i;
  b.__proto__ = i;
  bags.push(b);
}
show("lane bags", [Object.keys(bags[49]), bags[49].k3, bags[49].__proto__, Object.getPrototypeOf(bags[49])]);
const late = Object.create(null);
late.first = 0;
const hits: unknown[] = [];
Object.defineProperty(late, "k0", {
  set(v) {
    hits.push(v);
  },
  configurable: true,
  enumerable: true,
});
Object.defineProperty(late, "k1", { value: "ro", writable: false, enumerable: true, configurable: true });
late.k0 = 1;
attempt("lane late ro", () => {
  late.k1 = 2;
  return late.k1;
});
late.k2 = 3;
show("lane late", [hits, Object.keys(late), late.k1, late.k2]);
const relinked = Object.create(null);
relinked.first = 0;
Object.setPrototypeOf(relinked, {
  set k0(v: unknown) {
    hits.push("proto:" + v);
  },
});
relinked.k0 = 9;
show("lane relinked", [hits, Object.keys(relinked)]);
for (const b of bags.slice(0, 3)) {
  for (const k in b) b[k] = k;
}
show("lane forin", [bags[0], bags[2]]);
const single = Object.create(null);
single.only = 1;
const singleKeys: string[] = [];
for (const k in single) singleKeys.push(k);
const singleRelinked = Object.create(null);
singleRelinked.only = 1;
Object.setPrototypeOf(singleRelinked, { inheritedEnum: 1 });
const relinkedKeys: string[] = [];
for (const k in singleRelinked) relinkedKeys.push(k);
show("forin single", [singleKeys, relinkedKeys]);
