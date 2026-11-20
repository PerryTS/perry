"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.Doc = void 0;
class Doc {
    constructor(bson, offset = 0, isArray = false, elements) {
        this.cache = Object.create(null);
        this.bson = bson;
        this.elements = elements ?? [bson.length];
    }
    get(name) { return this.cache[name] ?? null; }
}
exports.Doc = Doc;
