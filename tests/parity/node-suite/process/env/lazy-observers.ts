// Taking `process.env` reads nothing; every way of observing its own keys must
// still see the live environment. Each observer runs in a FRESH child process,
// as the first thing that touches the env object, so an observer that reads
// field storage without going through the enumeration funnel prints false.
import { spawnSync } from "node:child_process";
import * as util from "node:util";

const modes = [
  "keys", "forin", "spread", "rest", "json", "clone", "inspect", "entries",
  "values", "assign", "names", "ownkeys", "in", "has", "get",
];

function observe(mode: string): Record<string, unknown> | string {
  const e: any = process.env;
  switch (mode) {
    case "keys": return Object.fromEntries(Object.keys(e).map((k) => [k, e[k]]));
    case "forin": { const o: any = {}; for (const k in e) o[k] = e[k]; return o; }
    case "spread": return { ...e };
    case "rest": { const { SNAPENV_NOPE, ...r } = e; return r; }
    case "json": return JSON.parse(JSON.stringify(e));
    case "clone": return structuredClone(e);
    case "inspect": return util.inspect(e, { depth: 0, maxArrayLength: null, breakLength: Infinity });
    case "entries": return Object.fromEntries(Object.entries(e));
    case "values": return { SNAPENV_K: Object.values(e).includes("x") ? "x" : undefined };
    case "assign": return Object.assign({}, e);
    case "names": return Object.fromEntries(Object.getOwnPropertyNames(e).map((k) => [k, e[k]]));
    case "ownkeys": return Object.fromEntries(Reflect.ownKeys(e).map((k: any) => [k, e[k]]));
    case "in": return { SNAPENV_K: "SNAPENV_K" in e ? "x" : undefined, SNAPENV_W: "SNAPENV_W" in e ? "1" : undefined };
    case "has": return {
      SNAPENV_K: Object.prototype.hasOwnProperty.call(e, "SNAPENV_K") ? "x" : undefined,
      SNAPENV_W: Object.prototype.hasOwnProperty.call(e, "SNAPENV_W") ? "1" : undefined,
    };
    default: return { SNAPENV_K: e.SNAPENV_K, SNAPENV_W: e.SNAPENV_W };
  }
}

function show(label: string, mode: string): void {
  const r = observe(mode);
  if (typeof r === "string") {
    console.log(label, "K", r.includes("SNAPENV_K: x"), "W", r.includes("SNAPENV_W: 1"));
  } else {
    console.log(label, "K", r.SNAPENV_K, "W", r.SNAPENV_W);
  }
}

const last = process.argv[process.argv.length - 1];
if (modes.includes(last)) {
  show("first", last);
  process.env.SNAPENV_W = "1";
  show("assigned", last);
  console.log("read-after-assign", process.env.SNAPENV_W, "SNAPENV_W" in process.env);
  delete process.env.SNAPENV_W;
  show("deleted", last);
  console.log("read-after-delete", process.env.SNAPENV_W, "SNAPENV_W" in process.env);
} else {
  for (const mode of modes) {
    const r = spawnSync(process.execPath, [...process.argv.slice(1), mode], {
      env: { ...process.env, SNAPENV_K: "x" },
      encoding: "utf8",
    });
    console.log("== " + mode + " status=" + r.status);
    console.log(String(r.stdout).trimEnd());
  }
}
