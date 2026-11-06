import { channel } from "node:diagnostics_channel";
import { AsyncLocalStorage } from "node:async_hooks";

const sync: string[] = [];

function describe(value: unknown) {
  return value === undefined ? "undefined" : JSON.stringify(value);
}

process.on("uncaughtException", (err: any) => {
  console.log("uncaught:", err?.name, err?.code || "no-code", err?.message);
});

function run(label: string, transform: unknown, bindWithArg = true) {
  const ch = channel(`dc-noncallable-transform-${label}`);
  const store = new AsyncLocalStorage();
  if (bindWithArg) {
    ch.bindStore(store, transform as any);
  } else {
    ch.bindStore(store);
  }
  const ret = ch.runStores({ value: label }, () => {
    sync.push(`${label} store:${describe(store.getStore())}`);
    return `${label}-ret`;
  });
  sync.push(`${label} ret:${ret}`);
}

run("null", null);
run("number", 1);
run("undefined", undefined);
run("omitted", undefined, false);

console.log("sync:", sync.join("|"));
setTimeout(() => console.log("done"), 0);
