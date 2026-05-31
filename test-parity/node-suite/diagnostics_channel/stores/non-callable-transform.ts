import { AsyncLocalStorage } from "node:async_hooks";
import { channel } from "node:diagnostics_channel";

const uncaught: string[] = [];
process.on("uncaughtException", (err: any) => {
  uncaught.push(`${err.name}:${err.code || "no-code"}:${err.message}`);
});

function run(label: string, hasTransform: boolean, transform?: any): void {
  const ch = channel(`dc-noncallable-transform-${label}`);
  const store = new AsyncLocalStorage();
  if (hasTransform) {
    ch.bindStore(store, transform);
  } else {
    ch.bindStore(store);
  }
  const ret = ch.runStores({ value: label }, () => {
    console.log(`${label} store:`, JSON.stringify(store.getStore()));
    return `ret-${label}`;
  });
  console.log(`${label} ret:`, ret);
}

run("null", true, null);
run("number", true, 1);
run("undefined", true, undefined);
run("omitted", false);

setImmediate(() => {
  console.log("uncaught:", uncaught.join("|"));
  console.log("after immediate");
});
