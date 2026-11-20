import { channel } from "node:diagnostics_channel";
const ch = channel("dc-data-identity");
const data = { ok: true };
ch.subscribe((seen: any) => console.log("same data:", seen === data));
ch.publish(data);
