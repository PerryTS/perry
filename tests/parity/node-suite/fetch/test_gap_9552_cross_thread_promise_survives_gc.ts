// #9552: a promise minted for a cross-thread settlement — every stdlib
// `fetch` response, among ~110 stdlib call sites — is referenced only by the
// worker's raw address while the request is in flight: the consumer's
// reaction hangs OFF the promise (`P.on_fulfilled`), nothing on the JS side
// points AT it. A malloc sweep landing in that window freed it; the slot was
// reused (in the report, by a RegExp header); and the completion then either
// saw a "settled" state byte and dropped the response — the request never
// resolves — or the microtask pump read the occupant as a promise (SIGSEGV).
//
// The constructor now pins the promise until it settles. Three consumer
// shapes each start a request against a local server that answers late,
// from a frame that has returned before any collection runs (so no native
// stack slot still names the promise); the churn then trips the
// malloc-count sweep with Symbols and reuses freed 80-byte slots with RegExp
// headers (the promise's size class). Unfixed, this hangs: at least one of
// the three never resolves. Node only exposes `gc` under --expose-gc, so
// that call is conditional and the expected output is identical on both.
import http from "node:http";

declare const gc: undefined | (() => void);

const server = http.createServer((_req, res) => {
  setTimeout(() => {
    res.end("ok");
  }, 1500);
});

class Client {
  async request(url: string): Promise<string> {
    const response = await fetch(url);
    return await response.text();
  }
}

server.listen(0, "127.0.0.1", () => {
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : 0;
  const url = `http://127.0.0.1:${port}/`;
  const results: Array<Promise<string>> = [];

  setTimeout(() => {
    // (a) no await at all: the reaction hangs off the fetch promise, nothing points at it
    results.push(fetch(url).then((r) => r.text()));
    // (b) async arrow
    const viaArrow = async () => {
      const r = await fetch(url);
      return await r.text();
    };
    results.push(viaArrow());
    // (c) async class method
    results.push(new Client().request(url));
  }, 0);

  let round = 0;
  const churn = () => {
    for (let i = 0; i < 40000; i++) {
      Symbol(`s${i & 255}`);
    }
    if (typeof gc === "function") gc();
    for (let i = 0; i < 4000; i++) {
      new RegExp(`r${i & 255}`);
    }
    round += 1;
    if (round < 6) {
      setTimeout(churn, 5);
      return;
    }
    Promise.all(results).then((bodies) => {
      console.log(bodies.join(","), round);
      server.close();
    });
  };
  setTimeout(churn, 20);
});
