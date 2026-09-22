'use strict';
// #10484: undici 8.x `lib/web/fetch/request.js` / webidl2js shapes. Every class
// in a compiled CommonJS package is constructed through a runtime value.

function argumentLengthCheck({ length }, min, ctx) {
  if (length < min) {
    throw new TypeError(`${ctx}: ${min} argument${min !== 1 ? 's' : ''} required, but only ${length} found.`);
  }
}

class Request {
  constructor(input, init = {}) {
    argumentLengthCheck(arguments, 1, 'Request constructor');
    this.url = input;
    this.init = init;
    this.argc = arguments.length;
  }
}

class Headers {
  constructor(init = undefined) {
    this.argc = arguments.length;
    this.list = Array.from(arguments);
  }
}

// A subclass that forwards `arguments` the way transpiled code does.
class StrictRequest extends Request {
  constructor() {
    super(...arguments);
    this.forwarded = arguments.length;
  }
}

// `fn.apply(this, arguments)` inside a constructor.
function record(self, a, b) {
  self.applied = [a, b];
}
class Applier {
  constructor(a, b) {
    record.apply(null, [this].concat(Array.prototype.slice.call(arguments)));
    this.argc = arguments.length;
  }
}

module.exports = { Request, Headers, StrictRequest, Applier, argumentLengthCheck };
