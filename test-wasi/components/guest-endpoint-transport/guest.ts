import { collect } from "perry/gc";
import { register } from "./generated/guest";

// The independent host supplies payload writers. These exports exercise both
// reader transport and actual JS guest reads; guest writers are separate work.
let started = 0;
let held;
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
  "read-started"() { collect(); return started; },
  async "read-future"(value) { started++; const result = await value.read(); collect(); return result; },
  async "read-character"(value) { started++; const result = await value.read(); collect(); return result; },
  async "read-empty"(value) { started++; return (await value.read()) === undefined; },
  async "read-values"(value) {
    started++;
    const result = [];
    for (;;) {
      const chunk = await value.read(2);
      collect();
      if (chunk.done) return result;
      for (const item of chunk.value) result.push(item);
    }
  },
  async "read-units"(value) {
    started++;
    let count = 0;
    for (;;) {
      const chunk = await value.read(2);
      collect();
      if (chunk.done) return count;
      for (const item of chunk.value) {
        if (item !== undefined) throw new Error("unit payload must be undefined");
        count++;
      }
    }
  },
  async "read-bytes"(value) {
    started++;
    const result = [];
    for (;;) {
      const chunk = await value.read(2);
      collect();
      if (chunk.done) return new Uint8Array(result);
      for (const item of chunk.value) result.push(item);
    }
  },
  async "read-nested"(value) {
    started++;
    const inner = await value.read();
    collect();
    return await inner.read();
  },
  async "dispose-pending"(value) {
    started++;
    const pending = value.read();
    value.dispose();
    try { await pending; return false; }
    catch (error) { return error.name === "AbortError"; }
  },
  async "abandon-read"(value) {
    held = value;
    value.read();
    await Promise.resolve();
    collect();
    return true;
  },
  async "read-held"() { started++; return await held.read(); },
});
