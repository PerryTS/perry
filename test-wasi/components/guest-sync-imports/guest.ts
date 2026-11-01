import { collect } from "perry/gc";
import { AsyncLocalStorage } from "node:async_hooks";
import { asyncTypes, register, imports } from "./generated/guest";

const foreign = imports["perry:guest-sync-imports/foreign@1.0.0"];
const storage = new AsyncLocalStorage<{ id: number }>();
let completed = 0;
let failed = 0;
let blocked = 0;
let guards = 0;
function context(id: number | undefined) {
  const actual = storage.getStore();
  if (id === undefined ? actual !== undefined : actual?.id !== id)
    throw new Error("staged import ALS isolation");
}
function factory() {
  for (const key of Object.keys(asyncTypes))
    if (asyncTypes[key].kind === "future") return asyncTypes[key];
  throw new Error("missing reply future factory");
}
async function produce(id, owner, loan, exchange, writer) {
  context(id);
  const returned = await exchange;
  collect(); context(id);
  if (foreign["[method]item.read"](returned.borrow()) !== id * 2 + 1)
    throw new Error("staged import owner identity");
  const value = await loan;
  collect(); context(id);
  if (value !== id * 2 || foreign["[method]item.read"](owner.borrow()) !== value)
    throw new Error("staged import loan identity");
  owner.dispose();
  const result = await writer.write({ owner: returned, context: id,
    text: "staged-" + id + " 日本語 🍐", bytes: new Uint8Array([0, 128, 255, id]) });
  collect(); context(id);
  if (result.written !== 1 || !result.closed || result.remaining.length !== 0)
    throw new Error("staged import future custody");
}
register({
  "perry:guest-sync-imports/api@1.0.0": {
    make(id) {
      context(undefined);
      return storage.run({ id }, () => {
        const owner = foreign["[constructor]item"](id * 2);
        const other = foreign["[constructor]item"](id * 2 + 1);
        const loan = foreign.loan(id, owner.borrow());
        const exchange = foreign.exchange(id, other);
        try { owner.dispose(); throw new Error("missing staged loan"); }
        catch (error) {
          if (error.message === "missing staged loan") throw error;
          blocked++;
        }
        foreign.seal(id);
        const pair = factory().create();
        produce(id, owner, loan, exchange, pair.writer).then(
          () => { completed++; }, () => { failed++; });
        return pair.value;
      });
    },
    "make-resource"(value) { context(undefined); return foreign["[constructor]item"](value); },
    "resource-value"(value) { context(undefined); collect(); return foreign["[method]item.read"](value); },
    guarded(value) {
      context(undefined);
      try { foreign.loan(9000, value); }
      catch { guards++; return 77; }
      throw new Error("incoming borrow escaped synchronous return");
    },
    checkpoint() { context(undefined); collect(); return { completed, failed, blocked, guards }; },
  },
});
