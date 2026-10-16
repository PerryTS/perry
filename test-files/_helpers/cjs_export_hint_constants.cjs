// Shaped like next/dist/shared/lib/constants.js: a dead `0 && (module.exports
// = {...: null})` export-name hint for static analysers, then getters
// installed by `_export`. Only the getters are live.
"use strict";
Object.defineProperty(exports, "__esModule", {
    value: true
});
0 && (module.exports = {
    BLOCKED_PAGES: null,
    BUILD_ID_FILE: null,
    CONFIG_FILES: null
});
function _export(target, all) {
    for(var name in all)Object.defineProperty(target, name, {
        enumerable: true,
        get: all[name]
    });
}
_export(exports, {
    BLOCKED_PAGES: function() {
        return BLOCKED_PAGES;
    },
    BUILD_ID_FILE: function() {
        return BUILD_ID_FILE;
    },
    CONFIG_FILES: function() {
        return CONFIG_FILES;
    }
});
const CONFIG_FILES = [
    "next.config.js",
    "next.config.mjs"
];
const BUILD_ID_FILE = "BUILD_ID";
const BLOCKED_PAGES = ["/_document", "/_app", "/_error"];
