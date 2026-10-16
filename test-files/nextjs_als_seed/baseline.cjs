// Next.js node-environment-baseline.js: expose AsyncLocalStorage on globalThis.
"use strict";
if (typeof globalThis.AsyncLocalStorage !== "function") {
  const { AsyncLocalStorage } = require("async_hooks");
  globalThis.AsyncLocalStorage = AsyncLocalStorage;
}
