'use strict';

function probe() {
    var out = [];
    out.push(T ? 'truthy' : 'falsy');
    out.push(typeof T);
    out.push(String(Array.isArray(T)));
    try { out.push(String(T.length)); } catch (e) { out.push(e.constructor.name); }
    try { out.push(String(T[1])); } catch (e) { out.push(e.constructor.name); }
    try { out.push(T.join('-')); } catch (e) { out.push(e.constructor.name); }
    return out.join(',');
}

function readNamed() {
    return probe();
}

// Both readers escape through `exports` before T is assigned.
exports.read = function read() { return probe(); };
module.exports.readNamed = readNamed;

var b = require('./cycle_b.cjs');
exports.early = b.early;
exports.earlyNamed = b.earlyNamed;

var T = (function () {
    var a = [];
    a.push('x');
    a.push('y');
    return a;
}());

exports.late = probe();
