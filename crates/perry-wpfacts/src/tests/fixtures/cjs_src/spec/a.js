'use strict';
const b = require('./b.js');
const proc = require('process');
module.exports = { Pt: b.Pt, mk: b.mk, argc: () => proc.argv.length };
