exports.loadedBefore = module.loaded;
exports.child = require("./child.cjs");
exports.children = module.children.map((child) => child.filename);
