const path = require("path");
const c = require("./c.cjs");
exports.name = c.name;
exports.children = () => module.children.map((m) => path.basename(m.id));
exports.cParent = () => path.basename(require.cache[require.resolve("./c.cjs")].parent.id);
