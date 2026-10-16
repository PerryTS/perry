// The bundled app-render async-local-storage.js shape: the constructor is
// snapshotted from globalThis at module scope, its statics called later.
"use strict";
let i = "undefined" != typeof globalThis && globalThis.AsyncLocalStorage;
exports.create = function () { return i ? new i() : null; };
exports.createSnapshot = function () { return i ? i.snapshot() : function (e, ...t) { return e(...t); }; };
exports.bindSnapshot = function (e) { return i ? i.bind(e) : e; };
exports.staticTypes = function () { return [typeof i, typeof i.snapshot, typeof i.bind]; };
