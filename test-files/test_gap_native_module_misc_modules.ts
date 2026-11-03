// req() of diagnostics_channel, trace_events, string_decoder, constants, sys,
// inspector/promises, node:test and node:test/reporters: every export must resolve
// to the real implementation, however it is read or called. node:test and its
// reporters are read through namespace imports; a literal require() of them
// does not yet resolve their exports.
import helper from "./gap_native_module_misc_modules_helper.cjs";
import * as nodeTest from "node:test";
import * as reporters from "node:test/reporters";
"use strict";
const { dc, dcPrefixed, te, tePrefixed, sd, sdPrefixed, constants, constantsPrefixed, fs, sys, util, inspector, ip, ipPrefixed } = helper;
const { channel, subscribe, unsubscribe, hasSubscribers } = dc;
const { StringDecoder } = sd;
const attempt = (fn) => { try { return fn(); } catch (e) { return "threw " + e.name; } };

for (const n of ["channel", "subscribe", "unsubscribe", "hasSubscribers", "tracingChannel"]) {
  console.log("diagnostics_channel", n, typeof dc[n], typeof dcPrefixed[n]);
}
console.log("diagnostics_channel same", dc === dcPrefixed, dc.channel === dcPrefixed.channel, channel === dc.channel);
const seen = [];
const onMessage = (message, name) => seen.push([name, message]);
console.log("hasSubscribers before", dc.hasSubscribers("nm-audit"));
dc.subscribe("nm-audit", onMessage);
console.log("hasSubscribers after", dc.hasSubscribers("nm-audit"), hasSubscribers("nm-audit"), dcPrefixed.hasSubscribers("nm-audit"));
const ch = dc.channel("nm-audit");
console.log("channel", typeof ch, ch.name, ch.hasSubscribers, dc.channel("nm-audit") === ch, channel("nm-audit") === ch);
ch.publish({ n: 1 });
(0, dc.channel)("nm-audit").publish({ n: 2 });
console.log("published", JSON.stringify(seen));
console.log("unsubscribe", dc.unsubscribe("nm-audit", onMessage), (0, unsubscribe)("nm-audit", onMessage), dc.hasSubscribers("nm-audit"));
subscribe("nm-audit-2", onMessage);
console.log("subscribe destructured", dc.hasSubscribers("nm-audit-2"));
const tracing = dc.tracingChannel("nm-audit-trace");
console.log("tracingChannel", typeof tracing, typeof tracing.traceSync, typeof tracing.subscribe);
console.log("traceSync", tracing.traceSync((a) => a + 1, {}, null, 2));

for (const n of ["createTracing", "getEnabledCategories"]) {
  console.log("trace_events", n, typeof te[n], typeof tePrefixed[n]);
}
const tracing2 = te.createTracing({ categories: ["node.perf"] });
console.log("createTracing", typeof tracing2, tracing2.enabled, JSON.stringify(tracing2.categories));
tracing2.enable();
console.log("enabled", tracing2.enabled, JSON.stringify(te.getEnabledCategories()));
tracing2.disable();
console.log("disabled", tracing2.enabled, te.getEnabledCategories());

console.log("string_decoder", typeof sd.StringDecoder, typeof sdPrefixed.StringDecoder, sd.StringDecoder === StringDecoder, sd.StringDecoder === sdPrefixed.StringDecoder);
const decoder = new sd.StringDecoder("utf8");
console.log("decode partial", JSON.stringify(decoder.write(Buffer.from([0xe2, 0x82]))), JSON.stringify(decoder.end(Buffer.from([0xac]))));
const decoder2 = new StringDecoder("hex");
console.log("decode hex", decoder2.write(Buffer.from("hi")), decoder2.encoding);
console.log("decode base64", new sdPrefixed.StringDecoder("base64").end(Buffer.from("abc")));

console.log("constants", typeof constants, constants === constantsPrefixed, typeof constants.O_RDONLY, constants.O_RDONLY === fs.constants.O_RDONLY,
  constants.O_WRONLY === fs.constants.O_WRONLY, constants.SIGINT, constants.SIGTERM, typeof constants.ENOENT);
console.log("constants keys", ["O_RDONLY", "SIGKILL", "S_IFMT", "ENOENT", "EACCES"].map((k) => typeof constants[k]).join());

console.log("sys", typeof sys.format, typeof sys.inspect, sys.format === util.format, sys.inspect === util.inspect);
console.log("sys format", sys.format("%s-%d", "a", 1), (0, sys.format)("%j", { a: 1 }), sys.isArray([]));

for (const n of ["Session", "open", "close", "url", "waitForDebugger", "console"]) {
  console.log("inspector/promises", n, typeof ip[n], typeof ipPrefixed[n]);
}
console.log("inspector/promises same", ip === ipPrefixed, ip.url === inspector.url, ip.open === inspector.open, ip.Session === inspector.Session);
console.log("inspector/promises url", ip.url(), (0, ipPrefixed.url)());
const session = new ip.Session();
console.log("inspector/promises Session", typeof session.connect, typeof session.post, typeof session.disconnect);

console.log("node:test", typeof (nodeTest as any).test, typeof (nodeTest as any).describe, typeof (nodeTest as any).it,
  typeof (nodeTest as any).before, typeof (nodeTest as any).after, typeof (nodeTest as any).beforeEach, typeof (nodeTest as any).afterEach, typeof (nodeTest as any).mock, typeof (nodeTest as any).run,
  typeof (nodeTest as any).suite, typeof (nodeTest as any).skip, typeof (nodeTest as any).todo, typeof (nodeTest as any).only);
console.log("node:test/reporters", ["spec", "tap", "dot", "junit", "lcov"].map((k) => typeof (reporters as any)[k]).join());
process.exit(0);
