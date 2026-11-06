// Minimal in-memory MongoDB wire-protocol server for gap tests that compile
// the real npm `mongodb` driver from source (no mongod in CI). Speaks just
// enough of OP_QUERY (legacy handshake) and OP_MSG to serve a single-server
// topology: hello, ping, insert, find, update, delete, aggregate-count,
// endSessions. Records every non-handshake command name it serves so a test
// can prove the driver really went over the wire.
import { BSON } from "mongodb";

const OP_REPLY = 1;
const OP_QUERY = 2004;
const OP_MSG = 2013;

function matches(doc: any, filter: any): boolean {
  for (const key of Object.keys(filter || {})) {
    const cond = filter[key];
    const v = doc[key];
    if (cond !== null && typeof cond === "object" && !Array.isArray(cond)) {
      for (const op of Object.keys(cond)) {
        const c = cond[op];
        if (op === "$lt" && !(v < c)) return false;
        if (op === "$lte" && !(v <= c)) return false;
        if (op === "$gt" && !(v > c)) return false;
        if (op === "$gte" && !(v >= c)) return false;
        if (op === "$eq" && v !== c) return false;
      }
    } else if (v !== cond) {
      return false;
    }
  }
  return true;
}

function applySort(docs: any[], sort: any): any[] {
  const keys = Object.keys(sort || {});
  if (keys.length === 0) return docs;
  const out = docs.slice();
  out.sort((x: any, y: any) => {
    for (const k of keys) {
      const dir = sort[k] < 0 ? -1 : 1;
      if (x[k] < y[k]) return -dir;
      if (x[k] > y[k]) return dir;
    }
    return 0;
  });
  return out;
}

// `net` is passed in by the test rather than imported here: the parity
// harness decides a fixture's compile mode (auto-optimize for modules routed
// to a perry-ext-* wrapper, such as `net`) from the TEST file's own imports,
// the same arrangement as fake_resp_server.ts.
export function startFakeMongo(net: any, onReady: (port: number, served: string[]) => void) {
  const collections: Record<string, any[]> = {};
  const served: string[] = [];
  const coll = (db: string, name: string) => {
    const ns = db + "." + name;
    if (!collections[ns]) collections[ns] = [];
    return collections[ns];
  };

  function helloReply(): any {
    return {
      helloOk: true,
      isWritablePrimary: true,
      ismaster: true,
      maxBsonObjectSize: 16777216,
      maxMessageSizeBytes: 48000000,
      maxWriteBatchSize: 100000,
      localTime: new Date(0),
      logicalSessionTimeoutMinutes: 30,
      connectionId: 1,
      minWireVersion: 0,
      maxWireVersion: 21,
      readOnly: false,
      ok: 1,
    };
  }

  function run(cmd: any): any {
    const name = Object.keys(cmd)[0];
    const db = cmd.$db || "admin";
    if (name === "hello" || name === "ismaster" || name === "isMaster") return helloReply();
    served.push(name);
    if (name === "ping" || name === "endSessions" || name === "killCursors") return { ok: 1 };
    if (name === "insert") {
      const docs = cmd.documents || [];
      const c = coll(db, cmd.insert);
      for (const d of docs) c.push(d);
      return { n: docs.length, ok: 1 };
    }
    if (name === "find") {
      const c = coll(db, cmd.find);
      let docs = applySort(c.filter((d: any) => matches(d, cmd.filter)), cmd.sort);
      if (cmd.limit) docs = docs.slice(0, Math.abs(cmd.limit));
      return { cursor: { firstBatch: docs, id: BSON.Long.fromNumber(0), ns: db + "." + cmd.find }, ok: 1 };
    }
    if (name === "update") {
      const c = coll(db, cmd.update);
      let n = 0;
      let nModified = 0;
      for (const u of cmd.updates || []) {
        for (const d of c) {
          if (!matches(d, u.q)) continue;
          n++;
          const set = (u.u && u.u.$set) || {};
          let changed = false;
          for (const k of Object.keys(set)) {
            if (d[k] !== set[k]) { d[k] = set[k]; changed = true; }
          }
          if (changed) nModified++;
          if (!u.multi) break;
        }
      }
      return { n, nModified, ok: 1 };
    }
    if (name === "delete") {
      const ns = db + "." + cmd.delete;
      let n = 0;
      for (const del of cmd.deletes || []) {
        const keep: any[] = [];
        let removed = 0;
        for (const d of coll(db, cmd.delete)) {
          if (matches(d, del.q) && (del.limit === 0 || removed === 0)) { removed++; } else { keep.push(d); }
        }
        collections[ns] = keep;
        n += removed;
      }
      return { n, ok: 1 };
    }
    if (name === "aggregate") {
      // countDocuments: [{$match}, {$skip?}, {$limit?}, {$group: {_id: 1, n: {$sum: 1}}}]
      const c = coll(db, cmd.aggregate);
      let docs = c;
      for (const stage of cmd.pipeline || []) {
        if (stage.$match) docs = docs.filter((d: any) => matches(d, stage.$match));
        if (stage.$group) docs = docs.length ? [{ _id: 1, n: docs.length }] : [];
      }
      return { cursor: { firstBatch: docs, id: BSON.Long.fromNumber(0), ns: db + "." + cmd.aggregate }, ok: 1 };
    }
    return { ok: 0, errmsg: "fake server: unsupported command " + name, code: 59 };
  }

  function header(len: number, requestId: number, responseTo: number, opCode: number): Buffer {
    const h = Buffer.alloc(16);
    h.writeInt32LE(len, 0);
    h.writeInt32LE(requestId, 4);
    h.writeInt32LE(responseTo, 8);
    h.writeInt32LE(opCode, 12);
    return h;
  }

  let nextId = 1;
  const server = net.createServer((sock: any) => {
    let pending = Buffer.alloc(0);
    sock.on("data", (chunk: Buffer) => {
      pending = Buffer.concat([pending, chunk]);
      while (pending.length >= 16) {
        const len = pending.readInt32LE(0);
        if (pending.length < len) break;
        const msg = pending.subarray(0, len);
        pending = pending.subarray(len);
        const requestId = msg.readInt32LE(4);
        const opCode = msg.readInt32LE(12);
        if (opCode === OP_QUERY) {
          let off = 20;
          while (msg[off] !== 0) off++;
          off += 1 + 8; // cstring NUL, numberToSkip, numberToReturn
          const q = BSON.deserialize(msg.subarray(off, off + msg.readInt32LE(off)));
          const body = Buffer.from(BSON.serialize(run(q)));
          const fixed = Buffer.alloc(20);
          fixed.writeInt32LE(0, 0); // responseFlags
          // cursorID (8 bytes) stays 0
          fixed.writeInt32LE(0, 12); // startingFrom
          fixed.writeInt32LE(1, 16); // numberReturned
          sock.write(Buffer.concat([header(16 + 20 + body.length, nextId++, requestId, OP_REPLY), fixed, body]));
        } else if (opCode === OP_MSG) {
          let off = 20;
          let cmd: any = null;
          while (off < len) {
            const kind = msg[off];
            off += 1;
            const size = msg.readInt32LE(off);
            if (kind === 0) {
              cmd = BSON.deserialize(msg.subarray(off, off + size));
              off += size;
            } else {
              const end = off + size;
              let p = off + 4;
              const idStart = p;
              while (msg[p] !== 0) p++;
              const ident = msg.subarray(idStart, p).toString("utf8");
              p++;
              const docs: any[] = [];
              while (p < end) {
                const dl = msg.readInt32LE(p);
                docs.push(BSON.deserialize(msg.subarray(p, p + dl)));
                p += dl;
              }
              if (cmd) cmd[ident] = docs;
              off = end;
            }
          }
          const body = Buffer.from(BSON.serialize(run(cmd)));
          const flags = Buffer.alloc(5); // flagBits = 0, section kind 0
          sock.write(Buffer.concat([header(16 + 5 + body.length, nextId++, requestId, OP_MSG), flags, body]));
        } else {
          sock.destroy();
        }
      }
    });
    sock.on("error", () => {});
  });
  server.listen(0, "127.0.0.1", () => onReady((server.address() as any).port, served));
  return server;
}
