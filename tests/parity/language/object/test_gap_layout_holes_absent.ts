// Object model §3.2: a layout key whose slot holds the hole is ABSENT.
//
// `child_process.spawnSync`'s result has `error` first, and only when the
// spawn failed. Perry births it on one declared layout whose `error` slot is
// optional, so a successful run leaves that slot holding the hole. Every
// reader of presence, order and value must then see no `error` key, exactly
// as node does, while a failed run's `error` is an ordinary first key.
import { spawnSync } from "child_process";
import * as util from "util";

const ok: any = spawnSync("echo", ["hi"], { encoding: "utf8" });
const bad: any = spawnSync("perry-gap-no-such-command-12345", [], { encoding: "utf8" });

function line(label: string, value: unknown): void {
  console.log(label + ": " + JSON.stringify(value));
}

function forInKeys(o: any): string[] {
  const out: string[] = [];
  for (const k in o) out.push(k);
  return out;
}

function report(name: string, r: any): void {
  line(name + " keys", Object.keys(r));
  line(name + " names", Object.getOwnPropertyNames(r));
  line(name + " ownKeys", Reflect.ownKeys(r));
  line(name + " for-in", forInKeys(r));
  line(name + " values.length", Object.values(r).length);
  line(name + " entries", Object.entries(r).map((e) => e[0]));
  line(name + " descriptors", Object.keys(Object.getOwnPropertyDescriptors(r)));
  line(name + " in", "error" in r);
  line(name + " hasOwn", Object.hasOwn(r, "error"));
  line(name + " hasOwnProperty", Object.prototype.hasOwnProperty.call(r, "error"));
  line(name + " propertyIsEnumerable", Object.prototype.propertyIsEnumerable.call(r, "error"));
  line(name + " descriptor", Object.getOwnPropertyDescriptor(r, "error") === undefined);
  line(name + " error type", typeof r.error);
  const k = "err" + "or";
  line(name + " computed type", typeof r[k]);
  const { error, status } = r;
  line(name + " destructured", [typeof error, status]);
  line(name + " optional chain", r.error?.code ?? "none");
  line(name + " json keys", Object.keys(JSON.parse(JSON.stringify(r))));
  line(name + " json array replacer", JSON.stringify(r, ["error", "status", "signal"]));
  line(
    name + " json fn replacer",
    Object.keys(JSON.parse(JSON.stringify(r, (key, value) => (key === "pid" ? 0 : value)))),
  );
  line(name + " spread", Object.keys({ ...r }));
  line(name + " spread tail", Object.keys({ ...r, tail: 1 }));
  line(name + " assign", Object.keys(Object.assign({}, r)));
  line(name + " structuredClone", Object.keys(structuredClone(r)));
  line(name + " inspect", util.inspect(r).includes("error:"));
  line(name + " status", r.status === 0 ? "zero" : r.status === null ? "null" : "other");
}

report("ok", ok);
report("bad", bad);
line("bad error code", bad.error.code);

// One site reads both objects: a cache primed on one must not answer for the
// other (the layout word is the same).
function readError(o: any): string {
  return typeof o.error;
}
function hasError(o: any): boolean {
  return "error" in o;
}
function ownError(o: any): boolean {
  return Object.hasOwn(o, "error");
}
const seen: string[] = [];
for (let i = 0; i < 8; i++) {
  const o = i % 2 === 0 ? bad : ok;
  seen.push(readError(o) + "/" + hasError(o) + "/" + ownError(o));
}
line("site alternation", seen);
const seen2: string[] = [];
for (let i = 0; i < 8; i++) {
  const o = i < 4 ? ok : bad;
  seen2.push(readError(o) + "/" + hasError(o) + "/" + ownError(o));
}
line("site ok then bad", seen2);

// A read of an absent key continues to the prototype.
const viaProto: any = spawnSync("echo", ["hi"], { encoding: "utf8" });
Object.setPrototypeOf(viaProto, { error: "inherited" });
line("inherited read", viaProto.error);
line("inherited in", "error" in viaProto);
line("inherited hasOwn", Object.hasOwn(viaProto, "error"));
line("inherited for-in", forInKeys(viaProto));
line("inherited keys", Object.keys(viaProto));
line("inherited site", [readError(viaProto), readError(ok), readError(bad), readError(viaProto)]);

// Deleting the absent key succeeds and changes nothing.
const deletedHole: any = spawnSync("echo", ["hi"], { encoding: "utf8" });
line("delete hole", delete deletedHole.error);
line("delete hole keys", Object.keys(deletedHole));

// Filling the hole makes the key present (its enumeration position is a later
// concern, so only presence and value are checked here).
const filled: any = spawnSync("echo", ["hi"], { encoding: "utf8" });
filled.error = 5;
line("filled value", filled.error);
line("filled in", "error" in filled);
line("filled hasOwn", Object.hasOwn(filled, "error"));
line("filled count", Object.keys(filled).length);
line("filled sorted", Object.keys(filled).sort());

// Deleting another key keeps the hole absent.
const deleted: any = spawnSync("echo", ["hi"], { encoding: "utf8" });
delete deleted.status;
line("deleted keys", Object.keys(deleted));
line("deleted in", "error" in deleted);
line("deleted json keys", Object.keys(JSON.parse(JSON.stringify(deleted))));
