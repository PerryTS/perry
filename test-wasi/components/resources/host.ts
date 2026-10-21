import { readFileSync } from "node:fs";
import { collect } from "perry/gc";
import { load } from "./generated/bindings";

const bytes = readFileSync(process.argv[2]);
const dropped: number[] = [];
const original = { reason: "host destructor failure" };
function makeComponent() {
  return load(bytes, {
    "perry:resources/observe@1.0.0": {
      dropped(value: number) {
        collect();
        dropped.push(value);
        if (value === 99) throw original;
      }
    }
  });
}

const component = makeComponent();
const counters = component.exports["perry:resources/counters@1.0.0"];
const value = counters["[constructor]counter"](42);
const borrowed = value.borrow();
collect();
if (counters["[method]counter.read"](borrowed) !== 42) throw new Error("borrow mismatch");
value.dispose();
value.dispose();
let rejected = false;
try { counters["[method]counter.read"](borrowed); } catch (_) { rejected = true; }
if (!rejected) throw new Error("dead borrow was accepted");
const transferred = counters["[constructor]counter"](7);
counters.consume(transferred);
rejected = false;
try { counters.consume(transferred); } catch (_) { rejected = true; }
if (!rejected) throw new Error("duplicate ownership transfer was accepted");
counters["[constructor]counter"](9);
component.dispose();
component.dispose();
if (dropped.length !== 3 || dropped[0] !== 42 || dropped[1] !== 7 || dropped[2] !== 9) {
  throw new Error("resource destructor count or order mismatch");
}

// No host callback runs for value 13: this checks a guest trap independently
// of the original-exception path used by throwing JS imports.
for (const value of [13, 99]) {
  const failed = makeComponent();
  const resource = failed.exports["perry:resources/counters@1.0.0"]["[constructor]counter"](value);
  let threw = false;
  try { resource.dispose(); } catch (error) {
    threw = true;
    if (value === 99 && error !== original) throw new Error("original destructor exception lost");
  }
  if (!threw) throw new Error("destructor failure was swallowed");
  resource.dispose(); // Consumed even when its destructor failed; no second drop.
  failed.dispose();
}
if (dropped.length !== 4 || dropped[3] !== 99) throw new Error("failed destructor retried");
collect();
console.log("PASS Rust resources: ownership, borrow, transfer, exactly-once disposal, destructor traps");
