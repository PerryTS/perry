"use strict";
// Mirrors mongodb 7.5.0's lib/cmap/wire_protocol/on_demand/document.js: the
// class reads module-scope bindings, so it is a capturing class, and it
// constructs itself with FEWER arguments than its constructor declares.
Object.defineProperty(exports, "__esModule", { value: true });
exports.OnDemandDocument = void 0;
const BSONElementOffset = { type: 0, nameOffset: 1, nameLength: 2, offset: 3, length: 4 };
function parseToElementsToArray(bson, offset) { return [bson.length, offset]; }
class OnDemandDocument {
    constructor(bson, offset = 0, isArray = false, elements) {
        this.cache = Object.create(null);
        this.bson = bson;
        this.offset = offset;
        this.isArray = isArray;
        this.elements = elements ?? parseToElementsToArray(this.bson, offset);
    }
    child(offset) { return new OnDemandDocument(this.bson, offset + BSONElementOffset.offset); }
    childArray(offset) { return new OnDemandDocument(this.bson, offset, true); }
}
exports.OnDemandDocument = OnDemandDocument;
