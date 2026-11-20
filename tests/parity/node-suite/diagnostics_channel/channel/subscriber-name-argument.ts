import { channel } from "node:diagnostics_channel";
const ch = channel("dc-subscriber-name-argument");
ch.subscribe((_data: any, name: any) => console.log("name arg:", name));
ch.publish("payload");
