const { parentPort } = require("node:worker_threads");

function define(label, descriptor) {
  const key = `PERRY_WORKER_DESCRIPTOR_${label.toUpperCase().replace("-", "_")}`;
  try {
    Object.defineProperty(process.env, key, descriptor);
    return `${label}:ok:${process.env[key]}:${
      Object.keys(process.env).includes(key)
    }`;
  } catch (error) {
    return `${label}:${error.name}:${error.code || ""}`;
  }
}

parentPort.postMessage([
  define("missing-flags", { value: 42 }),
  define("accessor", { get: () => "getter", configurable: true }),
  define("valid", {
    value: 42,
    configurable: true,
    writable: true,
    enumerable: true,
  }),
]);
