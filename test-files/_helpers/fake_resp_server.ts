// A minimal in-process RESP2 server for the redis-client gap tests
// (ioredis / iovalkey / node-redis compiled from their npm source). It
// speaks just enough of the protocol for connect, SET/GET/INCR/DEL, a
// pipeline, MULTI/EXEC, SUBSCRIBE/PUBLISH and QUIT, so the fixtures run
// without an external redis-server — byte-identical under Node and Perry.

type Reply = string;

function simple(s: string): Reply { return "+" + s + "\r\n"; }
function error(s: string): Reply { return "-" + s + "\r\n"; }
function int(n: number): Reply { return ":" + n + "\r\n"; }
function bulk(s: string | null): Reply {
  if (s === null) return "$-1\r\n";
  return "$" + Buffer.byteLength(s) + "\r\n" + s + "\r\n";
}
function arr(items: Reply[]): Reply { return "*" + items.length + "\r\n" + items.join(""); }

// Parse as many complete RESP arrays of bulk strings as `buf` holds.
function parseCommands(buf: string): { cmds: string[][]; rest: string } {
  const cmds: string[][] = [];
  let pos = 0;
  while (pos < buf.length) {
    if (buf[pos] !== "*") {
      const eol = buf.indexOf("\r\n", pos);
      if (eol < 0) break;
      cmds.push(buf.slice(pos, eol).split(" "));
      pos = eol + 2;
      continue;
    }
    const eol = buf.indexOf("\r\n", pos);
    if (eol < 0) break;
    const n = parseInt(buf.slice(pos + 1, eol), 10);
    let p = eol + 2;
    const args: string[] = [];
    let complete = true;
    for (let i = 0; i < n; i++) {
      const le = buf.indexOf("\r\n", p);
      if (le < 0) { complete = false; break; }
      const len = parseInt(buf.slice(p + 1, le), 10);
      const start = le + 2;
      if (buf.length < start + len + 2) { complete = false; break; }
      args.push(buf.slice(start, start + len));
      p = start + len + 2;
    }
    if (!complete) break;
    cmds.push(args);
    pos = p;
  }
  return { cmds, rest: buf.slice(pos) };
}

// `net` is passed in by the caller so each gap fixture imports `node:net`
// itself: the parity harness classifies ext-routed fixtures by their own
// imports, and these need the auto-optimize (coherent net archive) path.
export function startFakeRedis(net: any, onReady: (port: number, close: () => void) => void): void {
  const store = new Map<string, string>();
  const subscribers = new Map<string, Set<any>>();
  const sockets = new Set<any>();

  function exec(sock: any, state: any, args: string[]): Reply {
    const name = args[0].toUpperCase();
    switch (name) {
      case "PING": return simple("PONG");
      case "INFO": return bulk("# Server\r\nredis_version:7.2.0\r\nredis_mode:standalone\r\nloading:0\r\n");
      case "CLIENT": case "SELECT": return simple("OK");
      case "HELLO": return error("ERR unknown command 'HELLO'");
      case "SET": store.set(args[1], args[2]); return simple("OK");
      case "GET": return bulk(store.has(args[1]) ? (store.get(args[1]) as string) : null);
      case "INCR": case "INCRBY": {
        const by = name === "INCR" ? 1 : parseInt(args[2], 10);
        const v = parseInt(store.get(args[1]) ?? "0", 10) + by;
        store.set(args[1], String(v));
        return int(v);
      }
      case "DEL": {
        let n = 0;
        for (const k of args.slice(1)) if (store.delete(k)) n++;
        return int(n);
      }
      case "PUBLISH": {
        const subs = subscribers.get(args[1]);
        let n = 0;
        if (subs) for (const s of subs) { s.write(arr([bulk("message"), bulk(args[1]), bulk(args[2])])); n++; }
        return int(n);
      }
      case "SUBSCRIBE": {
        let out = "";
        for (const ch of args.slice(1)) {
          if (!subscribers.has(ch)) subscribers.set(ch, new Set());
          subscribers.get(ch)!.add(sock);
          state.subs.add(ch);
          out += arr([bulk("subscribe"), bulk(ch), int(state.subs.size)]);
        }
        return out;
      }
      default: return error("ERR unknown command '" + args[0] + "'");
    }
  }

  const server = net.createServer((sock: any) => {
    sockets.add(sock);
    sock.setEncoding("utf8");
    const state = { buf: "", queue: null as string[][] | null, subs: new Set<string>() };
    sock.on("error", () => {});
    sock.on("close", () => {
      sockets.delete(sock);
      for (const ch of state.subs) subscribers.get(ch)?.delete(sock);
    });
    sock.on("data", (chunk: string) => {
      state.buf += chunk;
      const { cmds, rest } = parseCommands(state.buf);
      state.buf = rest;
      let out = "";
      let quit = false;
      for (const args of cmds) {
        const name = args[0].toUpperCase();
        if (name === "QUIT") { out += simple("OK"); quit = true; break; }
        if (name === "MULTI") { state.queue = []; out += simple("OK"); continue; }
        if (name === "EXEC" && state.queue) {
          const q = state.queue; state.queue = null;
          out += arr(q.map((a) => exec(sock, state, a)));
          continue;
        }
        if (state.queue) { state.queue.push(args); out += simple("QUEUED"); continue; }
        out += exec(sock, state, args);
      }
      if (out) sock.write(out);
      if (quit) sock.end();
    });
  });
  server.listen(0, "127.0.0.1", () => {
    onReady(server.address().port, () => {
      for (const s of sockets) s.destroy();
      server.close();
    });
  });
}
