import { collect } from "perry/gc";
import { register } from "./generated/guest";

// These operations exercise owned endpoint transport. Reads/writes happen in
// the independent host; this fixture does not claim JS guest endpoint I/O.
register({
  "pass-future"(value) { collect(); return value; },
  async "pass-stream"(value) {
    await Promise.resolve();
    collect();
    return value;
  },
  "pass-pair"(value) { collect(); return value; },
  async character(value) { collect(); await Promise.resolve(); return value; },
  async empty(value) { collect(); await Promise.resolve(); return value; },
  async units(value) { collect(); await Promise.resolve(); return value; },
  async bytes(value) { collect(); await Promise.resolve(); return value; },
  async "nested-value"(value) { collect(); await Promise.resolve(); return value; },
  dispose(value) { value.dispose(); value.dispose(); },
  forget(_value) {},
  checkpoint() { collect(); return 73; },
});
