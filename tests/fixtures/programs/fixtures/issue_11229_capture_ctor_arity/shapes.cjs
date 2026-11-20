"use strict";
const K = { tag: "K" };
function helper(x) { return "h" + x; }
function norm(v) {
    if (v === undefined) return "<undef>";
    if (Array.isArray(v)) return v.map(norm);
    if (v !== null && typeof v === "object") { const out = {}; for (const k of Object.keys(v)) out[k] = norm(v[k]); return out; }
    return v;
}
function show(o) { return JSON.stringify(norm(o)); }
class Doc {
    constructor(bson, offset = 0, isArray = false, elements) {
        this.n = arguments.length;
        this.bson = bson; this.offset = offset; this.isArray = isArray;
        this.elements = elements ?? [helper(bson), K.tag];
    }
    child(o) { return new Doc(this.bson, o); }
    childArr(o) { return new Doc(this.bson, o, true); }
    viaThisCtor(o) { return new this.constructor(this.bson, o); }
}
class Dep {
    constructor(a, b = a + 1, c = b * 2) { this.v = [a, b, c, arguments.length, K.tag]; }
    static make1(a) { return new Dep(a); }
    static make2(a, b) { return new Dep(a, b); }
}
class Rest {
    constructor(first, ...more) { this.v = [first, more, arguments.length, helper(1)]; }
    static none() { return new Rest(); }
    static one() { return new Rest(1); }
    static three() { return new Rest(1, 2, 3); }
    static spread(xs) { return new Rest(...xs); }
}
class Spread {
    constructor(a, b = "B", c) { this.v = [a, b, c, arguments.length, K.tag]; }
    static s(xs) { return new Spread(...xs); }
}
class Base2 {
    constructor(a, b = "b-default", c) { this.base = [a, b, c, arguments.length, K.tag]; }
}
class Mid extends Base2 {
    constructor(a) { super(a); this.mid = helper(a); }
}
class Leaf extends Mid {}
class Implicit extends Base2 {}
class Many {
    constructor(a, b, c, d, e) { this.v = [a, b, c, d, e, arguments.length, K.tag, helper(0)]; }
    static zero() { return new Many(); }
}
// The same shapes WITHOUT `arguments` in the constructor: those are the ones
// the call-site padding pass rewrites (a ctor reading `arguments` is skipped).
class DepNA {
    constructor(a, b = a + 1, c = b * 2) { this.v = [a, b, c, K.tag]; }
    static make1(a) { return new DepNA(a); }
    static make0() { return new DepNA(); }
}
class BaseNA {
    constructor(a, b = "b-default", c = helper(a)) { this.base = [a, b, c, K.tag]; }
}
class MidNA extends BaseNA {
    constructor(a) { super(a); this.mid = helper(a); }
    static again(a) { return new MidNA(a); }
}
class LeafNA extends MidNA {}
class RestNA {
    constructor(first, second = "S", ...more) { this.v = [first, second, more, helper(2)]; }
    static one() { return new RestNA(1); }
}
module.exports = { Doc, Dep, Rest, Spread, Base2, Mid, Leaf, Implicit, Many, DepNA, BaseNA, MidNA, LeafNA, RestNA, show, K };
