"use strict";
// Mirrors the shape of mongodb 7.5.0's lib/cmap/wire_protocol/responses.js:
// a static `make` on the base reads module-scope bindings (a namespace object,
// a function, a const table), and is inherited by subclasses that carry their
// OWN, differently laid out, module-scope captures.
Object.defineProperty(exports, "__esModule", { value: true });
const lib_1 = { parse(b) { return [b.length]; } };
const doc_1 = require("./doc.cjs");
const Off = { a: 0, b: 1 };
const PREFIX = "resp:";
function isErr(b, els) {
    for (let i = 0; i < els.length; i++) {
        if (els[i] === Off.b + 100) return true;
    }
    return b === "err";
}
class ErrorBox {
    constructor(message) { this.message = message; }
}
class Base extends doc_1.Doc {
    static make(bson) {
        const elements = (0, lib_1.parse)(bson);
        const isError = isErr(bson, elements);
        return isError ? new Base(bson, 0, false, elements) : new this(bson, 0, false, elements);
    }
    static describe() {
        return PREFIX + this.name + ":" + typeof isErr + ":" + typeof lib_1.parse + ":" + Off.b;
    }
    static get tag() {
        return PREFIX + this.name + "/" + new ErrorBox("x").constructor.name;
    }
}
exports.Base = Base;
class Sub extends Base {
    constructor() {
        super(...arguments);
        this._batch = null;
        this.iterated = 0;
    }
    get id() {
        try { return lib_1.parse(this.cursor); }
        catch (cause) { throw new ErrorBox(cause.message); }
    }
    static kind() { return "sub-" + PREFIX + typeof ErrorBox; }
}
exports.Sub = Sub;
class Sub2 extends Sub {
    get more() { return Off.a + String(doc_1.Doc.name); }
}
exports.Sub2 = Sub2;
class Leaf extends Sub2 {
    static leafOnly() { return isErr("err", [0]) + ":" + PREFIX; }
}
exports.Leaf = Leaf;
