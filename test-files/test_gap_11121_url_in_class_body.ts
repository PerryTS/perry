// #11121: the only URL use in this module lives inside class bodies, reached
// through a `require("node:url")` namespace value — the shape of
// `@redis/client`'s `static parseURL`. Auto-optimize's feature detection
// scanned module init + top-level functions but not `classes`, so it left
// `global-url` off; the dynamic `new ns.URL(u)` dispatcher arm was compiled
// out and every getter on the result threw "Value of URL.prototype.hostname
// called on an incompatible receiver". (Only observable in an auto-optimized
// build: the default full runtime always has the arm.)

const node_url_1 = require("node:url");

class RedisLikeClient {
  static parseURL(url: string) {
    const { hostname, port, protocol, username, password, pathname } =
      new node_url_1.URL(url);
    return { hostname, port, protocol, username, password, pathname };
  }

  base = new node_url_1.URL("https://example.com/a/b?x=1");

  get query() {
    const params = new node_url_1.URLSearchParams(this.base.search);
    return params.get("x");
  }

  resolve(rel: string) {
    return new node_url_1.URL(rel, this.base).href;
  }

  static report() {
    console.log(JSON.stringify(RedisLikeClient.parseURL("redis://user:pw@127.0.0.1:6379/2")));
    const c = new RedisLikeClient();
    console.log(c.base.hostname, c.base.pathname, c.query);
    console.log(c.resolve("../c#h"));
    const u = new node_url_1.URL("rediss://host.example:6390");
    console.log(u instanceof node_url_1.URL, Object.prototype.toString.call(u), u.port);
  }
}

// Module init names no URL token: everything above is inside the class.
RedisLikeClient.report();
