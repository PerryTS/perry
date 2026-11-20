// Expected-different: the linux-fast snapshot records this raw Node mismatch.
// Node expected output (keep this oracle, even while the gap is known):
// source frames: dependency
// Perry also names the entry main frame that is dispatching this import.
import { stack } from "./fixtures/module-stack-dependency.mts";
const frames = stack.split("\n").slice(1).filter(line => /\/(?:module-stack-dependency|test_gap_module_stack_imported_known_gap)\.(?:mts|ts)(?::\d+:\d+)?/.test(line));
console.log("source frames:", frames.map(line => line.includes("module-stack-dependency") ? "dependency" : "entry").join(" > "));
