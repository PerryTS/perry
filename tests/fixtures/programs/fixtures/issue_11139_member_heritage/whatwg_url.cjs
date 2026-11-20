"use strict";
// Vendored from whatwg-url 14.2.0: the generated lib/URL.js wrapper shape
// (brand symbol, `install` declaring `class URL` whose constructor returns a
// branded object built from `new.target.prototype`, brand-checked getters)
// and lib/webidl2js-wrapper.js (install onto a shared global, re-export).
const implSymbol = Symbol("wrapper");

class URLImpl {
  constructor(globalObject, constructorArgs) {
    const url = constructorArgs[0];
    const m = /^([a-z][a-z0-9+.-]*):\/\/([^/?#]*)([^?#]*)/.exec(url);
    if (!m) {
      throw new globalObject.TypeError(`Invalid URL: ${url}`);
    }
    this.href = url;
    this.protocol = m[1] + ":";
    this.host = m[2];
    this.pathname = m[3] || "/";
  }
}

exports.is = value => {
  return value !== null && typeof value === "object" &&
    Object.prototype.hasOwnProperty.call(value, implSymbol) &&
    value[implSymbol] instanceof URLImpl;
};

exports.setup = (wrapper, globalObject, constructorArgs = []) => {
  Object.defineProperty(wrapper, implSymbol, {
    value: new URLImpl(globalObject, constructorArgs),
    configurable: true
  });
  return wrapper;
};

exports.install = (globalObject, globalNames) => {
  class URL {
    constructor(url) {
      if (arguments.length < 1) {
        throw new globalObject.TypeError(
          `Failed to construct 'URL': 1 argument required, but only ${arguments.length} present.`
        );
      }
      const args = [];
      {
        let curArg = arguments[0];
        curArg = String(curArg);
        args.push(curArg);
      }
      return exports.setup(Object.create(new.target.prototype), globalObject, args);
    }

    get protocol() {
      const esValue = this !== null && this !== undefined ? this : globalObject;
      if (!exports.is(esValue)) {
        throw new globalObject.TypeError("'get protocol' called on an object that is not a valid instance of URL.");
      }
      return esValue[implSymbol].protocol;
    }

    get host() {
      const esValue = this !== null && this !== undefined ? this : globalObject;
      if (!exports.is(esValue)) {
        throw new globalObject.TypeError("'get host' called on an object that is not a valid instance of URL.");
      }
      return esValue[implSymbol].host;
    }

    get pathname() {
      const esValue = this !== null && this !== undefined ? this : globalObject;
      if (!exports.is(esValue)) {
        throw new globalObject.TypeError("'get pathname' called on an object that is not a valid instance of URL.");
      }
      return esValue[implSymbol].pathname;
    }

    toString() {
      const esValue = this;
      if (!exports.is(esValue)) {
        throw new globalObject.TypeError("'toString' called on an object that is not a valid instance of URL.");
      }
      return esValue[implSymbol].href;
    }
  }
  Object.defineProperty(globalObject, "URL", {
    configurable: true,
    writable: true,
    value: URL
  });
};

const sharedGlobalObject = { Array, Error, Object, Promise, String, TypeError };
exports.install(sharedGlobalObject, ["Window"]);
exports.URL = sharedGlobalObject.URL;
