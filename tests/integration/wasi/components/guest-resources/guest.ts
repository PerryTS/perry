import { collect } from "perry/gc";
import { imports, register, resources } from "./generated/guest";
const counter = resources["perry:guest-resources/counters@1.0.0"].counter;
const foreign = imports["perry:guest-resources/foreign@1.0.0"];
let escaped: Parameters<typeof counter.value>[0] | undefined;
let cleanupForeign: Parameters<typeof foreign["[method]token.read"]>[0] | undefined;
let cleanupLocal: Parameters<typeof counter.value>[0] | undefined;
let checkpointJobs = 0;
let checkpointImmediate: ReturnType<typeof setImmediate> | undefined;
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
function cleanupCounter(value: number) {
  return counter.create({ value }, payload => {
    collect();
    imports["perry:guest-resources/observe@1.0.0"].dropped((payload as {value:number}).value);
  });
}
function abandonImported() { foreign["[constructor]token"](601); }
function abandonLocal() { cleanupCounter(602); }
register({
  "perry:guest-resources/counters@1.0.0": {
    "start-cleanup"() {
      queueMicrotask(() => { checkpointJobs += 1; });
      checkpointImmediate = setImmediate(() => { checkpointJobs += 10; });
      abandonImported();
      abandonLocal();
      cleanupForeign = foreign["[constructor]token"](603).borrow();
      foreign.consume(foreign["[constructor]token"](604));
      const explicit = foreign["[constructor]token"](605);
      explicit.dispose(); explicit.dispose();
      cleanupLocal = cleanupCounter(606).borrow();
      collect();
      // Finalizers collect again at the export checkpoint. This new return
      // owner must stay rooted until the adapter transfers it to the host.
      return cleanupCounter(607);
    },
    "check-cleanup-borrows"() {
      check(checkpointJobs === 1, "export checkpoint must drain jobs without running immediates");
      clearImmediate(checkpointImmediate!);
      collect();
      return foreign["[method]token.read"](cleanupForeign!) === 603
        && (counter.value(cleanupLocal!) as {value:number}).value === 606;
    },
    "release-cleanup-borrows"() { cleanupForeign = undefined; cleanupLocal = undefined; collect(); },
    "sweep-cleanup"() { collect(); },
    "read-imported"(value) { collect(); return foreign["[method]token.read"](value); },
    "[constructor]counter"(value) {
      return counter.create({ value }, (payload) => {
        collect();
        const value = (payload as { value: number }).value;
        if (value === 13) throw new Error("destructor trap");
        imports["perry:guest-resources/observe@1.0.0"].dropped(value);
      });
    },
    "[method]counter.read"(value) {
      collect();
      return (counter.value(value) as { value: number }).value;
    },
    consume(value) { collect(); value.dispose(); value.dispose(); },
    "keep-borrow"(value) { escaped = value; collect(); },
    "check-expired"() {
      collect();
      try { counter.value(escaped!); return false; } catch (_) { return true; }
    },
    "exercise-imports"() {
      const first = foreign["[constructor]token"](17);
      const borrow = first.borrow();
      collect();
      check(foreign["[method]token.read"](borrow) === 17, "import borrow");
      let rejected = false;
      try { foreign["consume-pair"]({ first, second: first }); } catch (_) { rejected = true; }
      check(rejected, "duplicate own must fail before host invocation");
      check(foreign["[method]token.read"](borrow) === 17, "failed transfer consumed owner");
      check(foreign.consume(first) === 17, "import transfer");
      rejected = false;
      try { foreign["[method]token.read"](borrow); } catch (_) { rejected = true; }
      check(rejected, "transferred owner's borrow survived");
      rejected = false;
      try { foreign.consume(first); } catch (_) { rejected = true; }
      check(rejected, "transferred owner survived");
      const second = foreign["[constructor]token"](23);
      second.dispose(); second.dispose();
      const a = foreign["[constructor]token"](5), b = foreign["[constructor]token"](6);
      check(foreign["consume-pair"]({ first: a, second: b }) === 11, "nested ownership");
      collect();
      return 4;
    }
  }
});
