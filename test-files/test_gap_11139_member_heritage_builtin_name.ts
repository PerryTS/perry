// #11139: `class X extends ns.URL {}` extends a PROPERTY of `ns`, not the
// global URL. mongodb-connection-string-url declares
// `class URLWithoutHost extends whatwg_url_1.URL {}` and
// `class ConnectionString extends URLWithoutHost`, where whatwg-url's `URL` is
// a plain class that brands the object its constructor returns. Perry kept
// only the trailing name `URL` and routed `super(...)` to its native URL, so
// the package constructor never ran and every getter's brand check threw
// ("'get protocol' called on an object that is not a valid instance of URL"),
// failing `new MongoClient(uri)`. Both modules are vendored under
// fixtures/issue_11139_member_heritage/.
import {
  ConnectionString,
  URLWithoutHost,
  MongoParseError,
} from "./fixtures/issue_11139_member_heritage/connection_string.cjs";
import * as whatwg from "./fixtures/issue_11139_member_heritage/whatwg_url.cjs";

function tryIt(label: string, f: () => unknown): void {
  try {
    console.log(label, f());
  } catch (e: any) {
    console.log(label, "THROWS", e.message);
  }
}

tryIt("direct", () => new whatwg.URL("mongodb://h/db").protocol);
tryIt("implicit protocol", () => new URLWithoutHost("mongodb://h/db").protocol);
tryIt("implicit branded", () => whatwg.is(new URLWithoutHost("mongodb://h/db")));

const cs = new ConnectionString("mongodb://127.0.0.1:27017,other:27018/db?w=1");
tryIt("cs protocol", () => cs.protocol);
tryIt("cs host", () => cs.host);
tryIt("cs pathname", () => cs.pathname);
tryIt("cs hosts", () => cs.hosts.join(" "));
tryIt("cs string", () => String(cs));
tryIt("cs instanceof", () =>
  [
    cs instanceof ConnectionString,
    cs instanceof URLWithoutHost,
    cs instanceof whatwg.URL,
    cs instanceof URL,
  ].join(" "),
);
tryIt("cs parse error", () => {
  try {
    return new ConnectionString("mongodb://");
  } catch (e) {
    return [e instanceof MongoParseError, (e as Error).name, (e as Error).message].join(" | ");
  }
});

// The same member shape written in this module, with another name codegen
// routes as a built-in: a package class that happens to be called `Map`.
const lib: any = {
  Map: class Map {
    entries: string[] = [];
    constructor(first: string) {
      this.entries.push(first);
    }
    add(v: string) {
      this.entries.push(v);
      return this;
    }
  },
};
class Registry extends lib.Map {
  constructor() {
    super("seed");
  }
}
tryIt("member Map", () => new Registry().add("x").entries.join(","));
tryIt("member Map is not global Map", () => new Registry() instanceof Map);

// Controls: the real global, bare and through globalThis, stays built-in.
class GlobalUrl extends URL {
  kind = "bare";
}
tryIt("global URL", () => {
  const u = new GlobalUrl("https://example.com/a?b=1");
  return [u.pathname, u.search, u.kind, u instanceof URL].join(" ");
});
class GlobalThisUrl extends globalThis.URL {}
tryIt("globalThis URL", () => {
  const u = new GlobalThisUrl("https://example.com/p");
  return [u.host, u.pathname, u instanceof URL].join(" ");
});
