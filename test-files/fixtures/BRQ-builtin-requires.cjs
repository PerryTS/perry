const fs = require("fs");
module.exports = {
  fs,
  prefixed: require("node:fs"),
  process: require("process"),
  promises: require("fs/promises"),
};
