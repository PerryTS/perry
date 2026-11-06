// Regression: util.formatWithOptions(options, format, ...args) must be
// accepted by the manifest gate (#463) and delegate to util.format.
// Required by the `debug` npm package (top-1k downloads, transitive dep
// of express/socket.io). The first arg is a util.inspect options bag
// that the stub currently ignores; full options-passthrough is a
// follow-up.
import { formatWithOptions } from "node:util";

console.log(formatWithOptions({}, "Hello %s", "world"));
console.log(formatWithOptions({ colors: false }, "x=%s y=%d", "k", 7));
