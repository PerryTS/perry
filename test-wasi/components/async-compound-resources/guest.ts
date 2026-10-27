import { collect } from "perry/gc";
import { imports, register } from "./generated/guest";
const foreign = imports["test:async-compound-resources/foreign@1.0.0"];
let publications = 0;
let cancelledBorrow: any;
function check(ok: boolean): void { if (!ok) throw new Error("compound ownership invariant"); }
function read(value: any): number { return foreign["[method]item.read"](value.borrow()); }
register({
  async "cancel-exchange"(id, value) {
    // Stage multiple distinct owners before finding an alias later in the
    // aggregate. Failure must leave every input usable and consume nothing.
    const first = value.owners[0].first;
    const other = value.owners[0].second;
    const expected = read(first);
    const expectedOther = read(other);
    let rejected = false;
    try {
      foreign.exchange(id + 50, {
        owners: [{ first, second: other, label: "duplicate" }],
        fallback: first, choice: value.choice
      });
    } catch (error) {
      rejected = String(error).includes("transferred more than once");
    }
    check(rejected);
    collect();
    check(read(first) === expected && read(other) === expectedOther);
    check(read(value.owners[1].first) === expected + 3);
    check(read(value.owners[1].second) === expected + 4);
    check(read(value.fallback) === expected + 5);
    imports.rejected(id);
    const pending = foreign.exchange(id, value);
    imports.submitted(id);
    const result = await pending;
    collect(); publications++;
    return result;
  },
  async loan(id, value) {
    await imports.wait(id);
    collect(); return foreign["[method]item.read"](value);
  },
  async "cancel-loan"(id, value) {
    cancelledBorrow = value;
    await imports.wait(id);
    collect(); return foreign["[method]item.read"](value);
  },
  "cancelled-expired"() {
    collect();
    try { foreign["[method]item.read"](cancelledBorrow); return false; }
    catch (_) { return true; }
  },
  published() { return publications; }
});
