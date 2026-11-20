import { channel, tracingChannel } from "node:diagnostics_channel";

const byName = tracingChannel("dc-trace");
console.log("start name:", byName.start.name);
console.log("end name:", byName.end.name);
console.log("asyncStart name:", byName.asyncStart.name);
console.log("asyncEnd name:", byName.asyncEnd.name);
console.log("error name:", byName.error.name);
const byObject = tracingChannel({
  start: channel("custom-start"),
  end: channel("custom-end"),
  asyncStart: channel("custom-asyncStart"),
  asyncEnd: channel("custom-asyncEnd"),
  error: channel("custom-error"),
});
console.log("object start name:", byObject.start.name);
console.log("object hasSubscribers:", byObject.hasSubscribers);
