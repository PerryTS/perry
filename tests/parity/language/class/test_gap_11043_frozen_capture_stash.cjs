// #11043: a CommonJS class with a computed member keeps its captured module
// bindings on each instance. Re-stashing them after a constructor that froze
// `this` threw "Cannot assign to read only property '__perry_cap_…'"
// (mongodb's HostAddress, reached from every MongoClient).
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const errors = { ParseError: class ParseError extends Error {} };
class HostAddress {
  constructor(hostString) {
    this.host = hostString;
    if (hostString.endsWith(".sock")) {
      this.socket = true;
      return;
    }
    if (hostString === "") {
      throw new errors.ParseError("empty host");
    }
    Object.freeze(this);
  }
  [Symbol.for("nodejs.util.inspect.custom")]() {
    return `HostAddress(${this.host})`;
  }
  toString() {
    return this.host;
  }
  static fromString(s) {
    return new HostAddress(s);
  }
}

const a = HostAddress.fromString("127.0.0.1:27017");
console.log(a.toString(), Object.isFrozen(a), JSON.stringify(Object.keys(a)));
const b = ["x:1", "y:2"].map(HostAddress.fromString);
console.log(b.map(String).join(" "), Object.isFrozen(b[1]));
const s = new HostAddress("/tmp/m.sock");
console.log(s.toString(), s.socket, Object.isFrozen(s));
try {
  new HostAddress("");
} catch (e) {
  console.log(e instanceof errors.ParseError, e.message);
}

// A capture the constructor changes still reaches the methods with its final
// value.
let made = 0;
class Counter {
  constructor() {
    made++;
    this.at = made;
  }
  [Symbol.iterator]() {
    return [this.at, made][Symbol.iterator]();
  }
}
new Counter();
const c = new Counter();
console.log(JSON.stringify([...c]));

// A write inside a declaration initializer is a change as well.
let built = 0;
class Tally {
  constructor() {
    const current = ++built;
    this.at = current;
  }
  [Symbol.iterator]() {
    return [this.at, built][Symbol.iterator]();
  }
}
new Tally();
console.log(JSON.stringify([...new Tally()]));
