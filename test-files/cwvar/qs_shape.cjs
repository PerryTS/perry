'use strict';

var limit = 1024;

// Hoisted reader declared before the table exists; only called after.
function at(i) {
    return hexTable[i];
}

var hexTable = (function () {
    var array = [];
    for (var i = 0; i < 256; ++i) {
        array.push('%' + ((i < 16 ? '0' : '') + i.toString(16)).toUpperCase());
    }
    return array;
}());

var encode = function encode(str) {
    var out = '';
    for (var i = 0; i < str.length; ++i) {
        var c = str.charCodeAt(i);
        if ((c >= 0x30 && c <= 0x39) || (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A)) {
            out += str.charAt(i);
            continue;
        }
        if (c < 0x80) {
            out += hexTable[c];
            continue;
        }
        if (c < 0x800) {
            out += hexTable[0xC0 | (c >> 6)] + hexTable[0x80 | (c & 0x3F)];
            continue;
        }
        out += hexTable[0xE0 | (c >> 12)]
            + hexTable[0x80 | ((c >> 6) & 0x3F)]
            + hexTable[0x80 | (c & 0x3F)];
    }
    return out;
};

module.exports = {
    encode: encode,
    at: at,
    size: function () { return hexTable.length; },
    kind: function () { return typeof hexTable + ' ' + Array.isArray(hexTable); },
    limit: limit
};
