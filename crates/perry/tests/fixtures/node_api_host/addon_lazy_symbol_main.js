// An addon whose unused path references a symbol nothing defines loads with
// the default binding (Node: RTLD_LAZY), and an explicit RTLD_NOW rejects it.
const os = require("os");
const path = require("path");
const file = path.join(process.cwd(), "node_modules/fixture-lazy-symbol/addon.node");
const strict = { exports: {} };
try {
  process.dlopen(strict, file, os.constants.dlopen.RTLD_NOW);
  console.log("RTLD_NOW", "loaded");
} catch (error) {
  console.log("RTLD_NOW", "rejected", String(error.message).includes("perry_fixture_never_defined"));
}
const addon = require("fixture-lazy-symbol");
console.log("default", typeof addon.unused, addon.ping());
