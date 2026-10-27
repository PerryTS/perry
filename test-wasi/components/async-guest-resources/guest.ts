import { collect } from "perry/gc";
import { imports, register, resources } from "./generated/guest";

const foreign = imports["test:async-guest-resources/foreign@1.0.0"];
const cell = resources["test:async-guest-resources/local@1.0.0"].cell;
let lastOwner: any;
let lastBorrow: any;
let lastLocalBorrow: any;
let tailExpired = false;
let publications = 0;
let cancelledBorrow: any;
function check(ok: boolean, message: string): void {
  if (!ok) throw new Error(message);
}
register({
    async create(value) {
      lastOwner = foreign["[constructor]item"](value);
      collect(); return lastOwner;
    },
    async echo(value) { collect(); lastOwner = value; return value; },
    async exchange(id, value) {
      const result = await foreign.exchange(id, value);
      collect(); lastOwner = result; return result;
    },
    async "cancel-exchange"(id, value) {
      const pending = foreign.exchange(id, value);
      imports.submitted(id);
      const result = await pending;
      publications++;
      collect(); return result;
    },
    published() { return publications; },
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
    async loan(id, value) {
      lastBorrow = value;
      await imports.wait(id);
      collect(); return foreign["[method]item.read"](value);
    },
    async loans(id, values) {
      await imports.wait(id);
      collect();
      const output: number[] = [];
      for (let i = 0; i < values.length; i++) {
        output.push(foreign["[method]item.read"](values[i]));
      }
      lastBorrow = values[0]; return output;
    },
    async "loan-background"(id, value) {
      lastBorrow = value;
      imports.wait(id).then(() => {
        collect();
        try { foreign["[method]item.read"](value); tailExpired = false; }
        catch (_) { tailExpired = true; }
      });
      collect(); return foreign["[method]item.read"](value);
    },
    "tail-expired"() { return tailExpired; },
    async "import-loan"(id, value) {
      const waiting = foreign["borrow-wait"](id, value.borrow());
      let refused = false;
      try { value.dispose(); } catch (_) { refused = true; }
      check(refused, "owner disposal must wait for canonical borrow acknowledgement");
      collect();
      const result = await waiting;
      collect(); value.dispose(); return result;
    },
    expired() {
      collect();
      try { foreign["[method]item.read"](lastBorrow); return false; }
      catch (_) { return true; }
    },
    transferred() {
      collect();
      try { lastOwner.borrow(); return false; } catch (_) { return true; }
    },
  "test:async-guest-resources/local@1.0.0": {
    async make(id) {
      await imports.wait(id);
      collect(); return cell.create({ id });
    },
    async read(id, value) {
      lastLocalBorrow = value;
      await imports.wait(id);
      collect(); return (cell.value(value) as {id: number}).id;
    },
    expired() {
      collect();
      try { cell.value(lastLocalBorrow); return false; } catch (_) { return true; }
    }
  }
});
