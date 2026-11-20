// The literal require() calls below are what make the compiler link each module.
module.exports = {
  dc: require("diagnostics_channel"),
  dcPrefixed: require("node:diagnostics_channel"),
  te: require("trace_events"),
  tePrefixed: require("node:trace_events"),
  sd: require("string_decoder"),
  sdPrefixed: require("node:string_decoder"),
  constants: require("constants"),
  constantsPrefixed: require("node:constants"),
  fs: require("fs"),
  sys: require("sys"),
  util: require("util"),
  inspector: require("inspector"),
  ip: require("inspector/promises"),
  ipPrefixed: require("node:inspector/promises"),
};
