'use strict';

var a = require('./cycle_a.cjs');
exports.early = a.read();
exports.earlyNamed = a.readNamed();
