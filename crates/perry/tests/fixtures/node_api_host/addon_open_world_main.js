const addon = require("fixture-open-world")
console.log("open-function", addon.run("return 6 * 7"))
const os = addon.builtin("os")
console.log("open-builtin", typeof os.platform === "function" && os.platform() === process.platform)
