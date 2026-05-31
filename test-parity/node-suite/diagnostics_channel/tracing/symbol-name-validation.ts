import * as dc from "node:diagnostics_channel";

function probe(label: string, value: symbol): void {
  try {
    dc.tracingChannel(value as any);
    console.log(`${label}: no throw`);
  } catch (err: any) {
    console.log(`${label}:`, err.name, err.code, err.message.split("\n")[0]);
  }
}

probe("symbol", Symbol("s"));
probe("symbolFor", Symbol.for("shared"));

const plain = dc.channel(Symbol("plain"));
console.log("plain symbol channel:", typeof plain, plain.hasSubscribers);
