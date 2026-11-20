// The literal require() calls below are what make the compiler link each module.
module.exports = {
  stream: require("stream"),
  bare: require("stream/promises"),
  prefixed: require("node:stream/promises"),
};
