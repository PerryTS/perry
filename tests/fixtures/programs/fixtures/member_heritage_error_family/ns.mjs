// Effect's internal/core.js shape: module bindings named like JS built-ins
// that hold package classes made by an IIFE (`export const Error = ...`).
export const Error = (function () {
  return class Base extends globalThis.Error {
    constructor(args) {
      super(args?.message);
      if (args) Object.assign(this, args);
    }
  };
})();

export const TypeError = (function () {
  return class Plain {
    constructor(v) {
      this.v = v;
    }
  };
})();

export const Event = (function () {
  return class PackageEvent {
    constructor(type) {
      this.kind = "pkg:" + type;
    }
  };
})();

export const Response = (function () {
  return class PackageResponse {
    constructor(body) {
      this.payload = body;
    }
  };
})();
