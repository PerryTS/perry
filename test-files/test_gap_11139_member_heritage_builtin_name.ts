// #11139: `class X extends ns.URL {}` extends ns's property, not the global
// URL. mongodb-connection-string-url declares
// `class URLWithoutHost extends whatwg_url_1.URL {}` and
// `class ConnectionString extends URLWithoutHost`, where whatwg-url's `URL` is
// a plain class that brands its instances. Perry kept only the trailing name
// `URL` and routed `super(...)` to its native URL, so the package constructor
// never ran and every getter's brand check threw. The shapes below are
// vendored from whatwg-url 14.2.0 (lib/URL.js wrapper) and
// mongodb-connection-string-url 7.0.2 (lib/index.js).

const implSymbol = Symbol("wrapper");

class URLImpl {
  href: string;
  constructor(url: string) {
    const m = /^([a-z+]+):\/\/([^/?#]*)(\/[^?#]*)?/.exec(url);
    if (!m) throw new TypeError(`Invalid URL: ${url}`);
    this.href = url;
    (this as any).protocol = m[1] + ":";
    (this as any).host = m[2];
    (this as any).pathname = m[3] || "/";
  }
}

// whatwg-url declares its wrapper class inside `install(globalObject)` and
// builds the instance itself: the constructor returns a fresh object whose
// prototype is `new.target.prototype`, branded with an own symbol property.
const whatwg_url_1: any = {};
whatwg_url_1.is = (value: any) =>
  value !== null &&
  typeof value === "object" &&
  Object.prototype.hasOwnProperty.call(value, implSymbol) &&
  value[implSymbol] instanceof URLImpl;
whatwg_url_1.setup = (wrapper: any, args: string[]) => {
  Object.defineProperty(wrapper, implSymbol, { value: new URLImpl(args[0]), configurable: true });
  return wrapper;
};
whatwg_url_1.install = (globalObject: any) => {
  class URL {
    constructor(url: string) {
      if (arguments.length < 1) {
        throw new globalObject.TypeError("Failed to construct 'URL': 1 argument required.");
      }
      return whatwg_url_1.setup(Object.create(new.target.prototype), [String(url)]);
    }
    get protocol(): string {
      if (!whatwg_url_1.is(this)) {
        throw new globalObject.TypeError("'get protocol' called on an object that is not a valid instance of URL.");
      }
      return (this as any)[implSymbol].protocol;
    }
    get pathname(): string {
      if (!whatwg_url_1.is(this)) {
        throw new globalObject.TypeError("'get pathname' called on an object that is not a valid instance of URL.");
      }
      return (this as any)[implSymbol].pathname;
    }
    toString(): string {
      return (this as any)[implSymbol].href;
    }
  }
  return URL;
};
whatwg_url_1.URL = whatwg_url_1.install(globalThis);

function tryIt(label: string, f: () => unknown): void {
  try {
    console.log(label, f());
  } catch (e: any) {
    console.log(label, "THROWS", e.message);
  }
}

// The package's exact inheritance: an implicit-constructor member subclass,
// then a subclass with a class field and `super(...)` inside `try`.
class URLWithoutHost extends whatwg_url_1.URL {}
class MongoParseError extends Error {}
class ConnectionString extends URLWithoutHost {
  _hosts: string[];
  constructor(uri: string) {
    const match = /^(mongodb(?:\+srv)?):\/\/([^/?]*)(.*)$/.exec(uri);
    if (!match) throw new MongoParseError("Invalid connection string");
    try {
      super(`${match[1]}://__this_is_a_placeholder__${match[3]}`);
    } catch (err: any) {
      throw new MongoParseError(err.message);
    }
    this._hosts = match[2].split(",");
  }
  get hosts(): string[] {
    return this._hosts;
  }
}

tryIt("implicit protocol", () => new URLWithoutHost("mongodb://h/db").protocol);
tryIt("implicit branded", () => whatwg_url_1.is(new URLWithoutHost("mongodb://h/db")));
const cs = new ConnectionString("mongodb://127.0.0.1:27017,other:27018/db");
tryIt("cs protocol", () => cs.protocol);
tryIt("cs pathname", () => cs.pathname);
tryIt("cs hosts", () => cs.hosts.join(" "));
tryIt("cs string", () => String(cs));
tryIt("cs instanceof", () => [
  cs instanceof ConnectionString,
  cs instanceof URLWithoutHost,
  cs instanceof whatwg_url_1.URL,
  cs instanceof URL,
].join(" "));
tryIt("cs parse error", () => new ConnectionString("postgres://h/db"));

// Another name-keyed built-in reached through a member: a package `Map`.
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
tryIt("member Map is global Map", () => new Registry() instanceof Map);

// Controls: the real globals, bare and through globalThis, stay built-in.
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
