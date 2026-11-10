const path = require("path");
require("./c.cjs");
require("./c.cjs");
exports.children = () => module.children.map((m) => path.basename(m.id));
