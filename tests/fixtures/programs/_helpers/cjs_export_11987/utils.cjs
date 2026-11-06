"use strict";
var formats = require("./formats.cjs");
var arrayToObject = function arrayToObject(source) {
    var obj = {};
    for (var i = 0; i < source.length; ++i) obj[i] = source[i];
    return obj;
};
var merge = function merge(target, source) {
    if (Array.isArray(target)) return arrayToObject(target.concat([source]));
    return source;
};
module.exports = { arrayToObject: arrayToObject, merge: merge, format: formats.RFC3986 };
