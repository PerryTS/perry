'use strict';
Object.defineProperty(exports, '__esModule', { value: true });
exports.helper = exports.VERSION = void 0;
exports.VERSION = '1.2.3';
function helper(x) { return 'helper:' + x; }
exports.helper = helper;
class Widget { constructor(n) { this.n = n; } size() { return this.n * 2; } }
exports.Widget = Widget;
