"use strict";
// Lane D (NXPD): a read of a CJS export getter is answered by the receiver's
// layout. A getter whose body is exactly `return <binding>` reads as a load
// of that binding (live: a reassigned `let` is seen at the next read).
const _lit = require("../../../fixtures/programs/cjs_export_getter_probe/lit.cjs");
const _helper = require("../../../fixtures/programs/cjs_export_getter_probe/helper.cjs");

let s = 0;
for (let i = 0; i < 1000; i++) {
  s += (0, _lit.parseUrl)(i);
  s += (0, _helper.getA)(i);
  s += (0, _helper.getB)(i);
}
console.log(s);

// Live binding through the getter.
console.log(_helper.counter);
_helper.bump();
_helper.bump();
console.log(_helper.counter);

// The getters stay accessors to reflection.
const d = Object.getOwnPropertyDescriptor(_lit, "parseUrl");
console.log(typeof d.get, d.enumerable, d.configurable, "value" in d);
console.log(Object.keys(_lit).join(","), Object.keys(_helper).join(","));
