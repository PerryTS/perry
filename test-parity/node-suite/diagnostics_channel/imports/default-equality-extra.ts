import dc, { channel, hasSubscribers } from "node:diagnostics_channel";
console.log("channel equality:", dc.channel === channel);
console.log("has equality:", dc.hasSubscribers === hasSubscribers);
console.log("default channel works:", dc.channel("dc-default-extra") === channel("dc-default-extra"));
