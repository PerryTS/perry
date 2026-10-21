import { collect } from "perry/gc";
import { imports, register, resources } from "./generated/guest";
const counter = resources["perry:guest-resources/counters@1.0.0"].counter;
const foreign = imports["perry:guest-resources/foreign@1.0.0"];
let escaped: Parameters<typeof counter.value>[0] | undefined;
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
register({
  "perry:guest-resources/counters@1.0.0": {
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
