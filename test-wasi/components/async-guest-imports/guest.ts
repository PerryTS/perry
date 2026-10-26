import { collect } from "perry/gc";
import { imports, register, type Exports } from "./generated/guest";

type Payload = {
  text: string; bytes: Uint8Array; owner: number;
  labels: { tag: "none" } | { tag: "some"; value: string[] };
};
function part(input: Payload, suffix: string): Payload {
  return { text: input.text + "/" + suffix, bytes: input.bytes, labels: input.labels, owner: input.owner };
}

const service = imports["test:async-guest-imports/service@1.0.0"];
let completed = 0;
let cancelledJobs = 0;
let cancelResolve: () => void = () => {};
let initializationGuard = 0;
try { imports.empty(); } catch { initializationGuard = 10000; }

const exports: Exports = {
  async run(id, input) {
    const first = await service.single(input);
    collect();
    const withGetter = {
      get text() { collect(); return first.text; },
      bytes: first.bytes, labels: first.labels, owner: first.owner
    };
    const result = await service.many(id, withGetter, part(input, "b"), part(first, "c"), part(input, "d"), part(first, "e"));
    collect();
    return {
      text: result.text + "/" + input.text + "/" + id,
      bytes: result.bytes, labels: result.labels,
      get owner() { collect(); return id; }
    };
  },
  async parallel(id, input) {
    const a = service.many(id, input, part(input, "b"), part(input, "c"), part(input, "d"), part(input, "e"));
    const other = part(input, "second");
    const b = service.many(id + 1, other, part(other, "b"), part(other, "c"), part(other, "d"), part(other, "e"));
    const second = await b;
    collect();
    const first = await a;
    return first.owner + second.owner;
  },
  async immediate() {
    const result = await imports.scalar(41);
    await imports.empty();
    collect();
    return result;
  },
  async background(id, input) {
    service.many(id, input, part(input, "b"), part(input, "c"), part(input, "d"), part(input, "e")).then(async (value) => {
      await imports.empty();
      collect();
      completed += value.owner;
    });
    input.text = "mutated after canonical parameter lowering";
    collect();
    return id;
  },
  guarded() {
    try { imports.empty(); } catch { return 77; }
    return 0;
  },
  status() { return initializationGuard + completed; },
  async "cancel-probe"(id, input) {
    // A sibling settles this after canonical cancellation and task EXIT.
    // Its reaction must retain the cancelled owner instead of being adopted.
    new Promise<void>(resolve => { cancelResolve = resolve; }).then(() => {
      collect();
      cancelledJobs += 1;
    });
    const a = service.many(id, input, part(input, "b"), part(input, "c"), part(input, "d"), part(input, "e"));
    const b = service.many(id + 1, input, part(input, "b"), part(input, "c"), part(input, "d"), part(input, "e"));
    await b;
    return await a;
  },
  "cancel-poke"() { cancelResolve(); collect(); return cancelledJobs; }
};
register(exports);
