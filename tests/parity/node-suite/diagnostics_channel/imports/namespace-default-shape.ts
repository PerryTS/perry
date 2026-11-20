import dcDefault, {
  Channel,
  channel,
  hasSubscribers,
  subscribe,
  tracingChannel,
  unsubscribe,
} from "node:diagnostics_channel";
import * as dc from "node:diagnostics_channel";

const expected = [
  "Channel",
  "channel",
  "hasSubscribers",
  "subscribe",
  "tracingChannel",
  "unsubscribe",
] as const;

console.log("namespace default type:", typeof dc.default);
console.log("default import identity:", dcDefault === dc.default);
console.log("default is namespace:", dcDefault === dc);
console.log(
  "default has own default:",
  Object.prototype.hasOwnProperty.call(dcDefault, "default"),
);
console.log("namespace publish:", typeof (dc as any).publish);
console.log("default publish:", typeof (dcDefault as any).publish);

for (const name of expected) {
  console.log(
    `${name}:`,
    typeof dc[name],
    typeof dcDefault[name],
    dc[name] === dcDefault[name],
  );
}

console.log("named identities:", channel === dc.channel, hasSubscribers === dc.hasSubscribers);
console.log("more named identities:", subscribe === dc.subscribe, unsubscribe === dc.unsubscribe);
console.log("tracing identity:", tracingChannel === dc.tracingChannel);
console.log("Channel identity:", Channel === dc.Channel);
