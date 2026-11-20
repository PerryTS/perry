// import of diagnostics_channel, string_decoder, trace_events and constants
// (namespace, default, named) must resolve every export to the real implementation.
import * as dc from "diagnostics_channel";
import dcDefault from "node:diagnostics_channel";
import { channel, hasSubscribers, subscribe, unsubscribe } from "diagnostics_channel";
import * as sd from "string_decoder";
import sdDefault from "node:string_decoder";
import { StringDecoder } from "string_decoder";
import * as te from "trace_events";
import teDefault from "node:trace_events";
import { getEnabledCategories } from "trace_events";
import constantsDefault from "constants";
import * as constantsNs from "node:constants";
import { constants } from "fs";

console.log("diagnostics_channel", typeof dc.channel, typeof dcDefault.channel, channel === dc.channel, dcDefault.subscribe === subscribe);
const seen = [];
const onMessage = (message, name) => seen.push([name, message]);
subscribe("esm-audit", onMessage);
console.log("hasSubscribers", hasSubscribers("esm-audit"), dc.hasSubscribers("esm-audit"), dcDefault.hasSubscribers("esm-audit"));
channel("esm-audit").publish("one");
dcDefault.channel("esm-audit").publish("two");
(0, dc.channel)("esm-audit").publish("three");
console.log("published", JSON.stringify(seen));
console.log("unsubscribe", unsubscribe("esm-audit", onMessage), dc.hasSubscribers("esm-audit"));
console.log("traceSync", dc.tracingChannel("esm-audit-trace").traceSync((a) => a * 2, {}, null, 21));

console.log("string_decoder", typeof sd.StringDecoder, typeof sdDefault.StringDecoder, StringDecoder === sd.StringDecoder);
const decoder = new sd.StringDecoder("utf8");
console.log("decode", JSON.stringify(decoder.write(Buffer.from([0xe2, 0x82]))), JSON.stringify(decoder.end(Buffer.from([0xac]))));
console.log("decode default", new sdDefault.StringDecoder("hex").write(Buffer.from("ok")));

console.log("trace_events", typeof te.createTracing, typeof teDefault.getEnabledCategories, getEnabledCategories === te.getEnabledCategories);
const tracing = te.createTracing({ categories: ["node.perf"] });
tracing.enable();
console.log("enabled", tracing.enabled, JSON.stringify(getEnabledCategories()));
tracing.disable();
console.log("disabled", tracing.enabled, teDefault.getEnabledCategories());

console.log("constants", typeof constantsDefault.O_RDONLY, constantsDefault.O_RDONLY === constants.O_RDONLY,
  constantsNs.default === constantsDefault, constantsDefault.SIGINT);
process.exit(0);
