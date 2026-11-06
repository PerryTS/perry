// Sloppy object methods follow the same mapping rules as ordinary functions.
function poke(args, value) { args[0] = value; }
const o = {
  mapped(a, b) {
    this.calls++;
    (() => { arguments[0] = b; })();
    console.log("method-arrow", a === b, arguments[0] === b, this.calls);
    a = 11;
    console.log("method-reverse", arguments[0]);
  },
  body(a, b) {
    arguments[0] = b;
    console.log("method-body", a === b);
  },
  escaped(a, b) {
    poke(arguments, b);
    console.log("method-helper", a === b);
  },
  strict(a) {
    "use strict";
    arguments[0] = 13;
    console.log("method-strict", a, arguments[0]);
  },
  defaulted(a = arguments[0]) {
    arguments[0] = 17;
    console.log("method-default", a === undefined, arguments[0], arguments.length);
  },
  rest(a, ...r) {
    arguments[0] = 19;
    console.log("method-rest", a, arguments[0], r.join(","));
  },
  destructured({value}) {
    arguments[0] = 23;
    console.log("method-destructure", value, arguments[0]);
  },
  missing(a, b) {
    arguments[1] = 29;
    console.log("method-missing", b === undefined, arguments[1]);
  },
  deleted(a) {
    delete arguments[0];
    arguments[0] = 31;
    console.log("method-deleted", a, arguments[0]);
  },
  calls: 0
};
o.mapped(1, 2);
o.body(1, 2);
o.escaped(1, 2);
o.strict(1);
o.defaulted();
o.rest(1, 2, 3);
o.destructured({value: 1});
o.missing(1);
o.deleted(1);
