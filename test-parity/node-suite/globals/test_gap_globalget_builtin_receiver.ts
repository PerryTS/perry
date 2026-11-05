// A CJS .js read must consult the actual builtin receiver, including misses.
import run from "../../../test-files/_helpers/globalget/builtin_values.js";
console.log(run());
