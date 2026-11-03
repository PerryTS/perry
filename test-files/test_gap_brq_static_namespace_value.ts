import * as fs from "node:fs";
import * as diagnostics from "node:diagnostics_channel";
import * as ordinary from "./fixtures/BRQ-esm-source.ts";
console.log(typeof fs, typeof fs.readFileSync, typeof fs.default.readFileSync);
console.log(typeof ordinary, ordinary.value);
const channel = diagnostics.tracingChannel("BRQ");
console.log(typeof diagnostics, typeof channel, channel.hasSubscribers);
