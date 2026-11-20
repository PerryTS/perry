import * as util from "node:util";

console.log("debuglog typeof:", typeof util.debuglog);
console.log("debug typeof:", typeof util.debug);
console.log("debug equals debuglog:", util.debug === util.debuglog);
console.log("debuglog length/name:", util.debuglog.length, util.debuglog.name);
console.log(
  "keys include:",
  Object.keys(util).includes("debuglog"),
  Object.keys(util).includes("debug"),
);

const numericLogger = util.debuglog(123, "not-callback");
console.log(
  "numeric logger:",
  typeof numericLogger,
  numericLogger.enabled,
  Object.keys(numericLogger).join(","),
);

const seen = [];
const logger = util.debuglog("perryprobe", (fn) => {
  seen.push(`${typeof fn}:${fn.enabled}`);
});
const wildcard = util.debug("foobar");

console.log("logger typeof:", typeof logger);
console.log("logger enabled:", logger.enabled);
console.log("logger keys:", Object.keys(logger).join(","));
const loggerNameDescriptor = Object.getOwnPropertyDescriptor(logger, "name");
console.log(
  "logger name descriptor:",
  loggerNameDescriptor?.value,
  loggerNameDescriptor?.writable,
  loggerNameDescriptor?.enumerable,
  loggerNameDescriptor?.configurable,
);
console.log("wildcard enabled:", wildcard.enabled);

const writes = [];
const originalWrite = process.stderr.write;
process.stderr.write = (chunk) => {
  writes.push(String(chunk).replace(String(process.pid), "PID"));
  return true;
};

try {
  console.log("callback before:", seen.join(",") || "none");
  logger("value=%d %s", 7, "ok");
  wildcard("other=%j", { a: 1 });
  console.log("callback after:", seen.join(",") || "none");
} finally {
  process.stderr.write = originalWrite;
}

console.log("writes:", JSON.stringify(writes));
