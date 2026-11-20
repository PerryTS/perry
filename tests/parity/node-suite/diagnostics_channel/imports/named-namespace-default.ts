import dcDefault, { channel, hasSubscribers, subscribe, tracingChannel, unsubscribe, Channel } from "node:diagnostics_channel";
import * as dc from "node:diagnostics_channel";

console.log("namespace channel type:", typeof dc.channel);
console.log("named channel identity:", channel === dc.channel);
console.log("default channel identity:", dcDefault.channel === dc.channel);
console.log("named hasSubscribers type:", typeof hasSubscribers);
console.log("named subscribe type:", typeof subscribe);
console.log("named unsubscribe type:", typeof unsubscribe);
console.log("named tracingChannel type:", typeof tracingChannel);
console.log("Channel type:", typeof Channel);
console.log("default Channel identity:", dcDefault.Channel === dc.Channel);
