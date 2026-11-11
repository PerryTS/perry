"use strict";
// SWC/TS `_export` helper: getters under computed keys from an object literal.
Object.defineProperty(exports, "__esModule", { value: true });
function _export(target, all) {
    for (var name in all) Object.defineProperty(target, name, { enumerable: true, get: all[name] });
}
_export(exports, {
    getA: function () { return getA; },
    getB: function () { return getB; },
    counter: function () { return counter; },
    bump: function () { return bump; },
});
function getA(x) { return x + 2; }
function getB(x) { return x + 3; }
let counter = 0;
function bump() { counter = counter + 1; }
