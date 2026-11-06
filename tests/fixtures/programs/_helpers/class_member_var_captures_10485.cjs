"use strict";
// Helper for test_gap_10485_class_member_var_captures.ts: the shape TypeScript
// emits for a class whose static members refer to the class itself
// (`var _a; class X { … _a … } _a = X;`), as in @redis/client 6.1.0's
// dist/lib/client/index.js. A CommonJS module body is a function scope, so
// every read of `_a` below is a class-member capture of a module-body `var`.
var _a;
var hits = 0;
function attachConfig({ BaseClass, tag }) {
  return class extends BaseClass {
    tagged() { return tag + ":" + this.describe(); }
  };
}
class Client {
  constructor(name) { this.name = name; hits++; }
  static create(name) { return _a.factory(name); }
  static factory(name) {
    const Commanded = attachConfig({ BaseClass: _a, tag: "cmd" });
    return new Commanded(name);
  }
  static hits() { return hits; }
  describe() { return "client(" + this.name + ") hits=" + _a.hits(); }
}
_a = Client;
exports.Client = Client;
exports.bumpHits = (n) => { hits += n; };
exports.readHits = () => hits;
