// Loads the addon the way prebuilt-addon packages do: a computed path
// through createRequire, resolved by the program's addon manifest.
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const build = "./build";
export default require(build + "/Release/second.node");
