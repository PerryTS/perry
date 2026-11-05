import { collect } from "perry/gc";
import { imports, register } from "./generated/guest";
const foreign = imports["test:async-compound-resources/foreign@1.0.0"];
let publications = 0;
let cancelledBorrow: any;
function check(ok: boolean, message: string): void {
  if (!ok) { console.error(message); throw new Error(message); }
}
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
    let failure = "duplicate request unexpectedly accepted";
    try {
      foreign.exchange(id + 50, {
        owners: [{ first, second: other, label: "duplicate" }],
        fallback: { tag: "some", value: first }, choice: value.choice
      });
    } catch (error) {
      failure = String(error);
      rejected = failure.includes("transferred more than once");
    }
    check(rejected, failure);
    collect();
    check(read(first) === expected && read(other) === expectedOther, "staged owners changed after rejection");
    check(read(value.owners[1].first) === expected + 3, "third owner changed");
    check(read(value.owners[1].second) === expected + 4, "fourth owner changed");
    check(value.fallback.tag === "some" && read(value.fallback.value) === expected + 5, "fallback owner changed");
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
