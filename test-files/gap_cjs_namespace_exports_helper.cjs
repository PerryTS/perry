"use strict";
// Top-level declarations this module does NOT export: a namespace import must
// not see them (node takes the names from what the module assigns to exports).
class Hidden {
    hi() { return "hidden"; }
}
class Base {
    base() { return "base"; }
}
class Pub extends Base {
    pub() { return new Hidden().hi(); }
}
class Boxed {
    boxed() { return "boxed"; }
}
function helperFn() { return "helperFn"; }
const helperConst = "helperConst";
function box(C) { return { inner: C }; }
exports.Pub = Pub;
exports.Alias = Base;
exports.Boxed = box(Boxed);
exports.run = () => helperFn() + "/" + helperConst;
