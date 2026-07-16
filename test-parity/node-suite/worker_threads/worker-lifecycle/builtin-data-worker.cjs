const { parentPort, workerData } = require("node:worker_threads");
const data = workerData || {};

parentPort.postMessage({
  date: data.date instanceof Date,
  dateValue: data.date instanceof Date ? data.date.toISOString() : undefined,
  map: data.map instanceof Map,
  mapValue: data.map instanceof Map ? data.map.get("key") : undefined,
  regexp: data.regexp instanceof RegExp,
  regexpValue: data.regexp instanceof RegExp ? data.regexp.source : undefined,
  bigintType: typeof data.bigint,
  bigintValue: String(data.bigint),
});
