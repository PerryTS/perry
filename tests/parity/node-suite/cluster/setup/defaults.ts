// Node v26.5.0 test-cluster-setup-primary-cumulative.js: setupPrimary fills
// exactly the four default settings when no options were previously supplied.
import cluster from "node:cluster";

console.log("before:", Object.keys(cluster.settings).join(","));
const returned = cluster.setupPrimary();
console.log("returns undefined:", returned === undefined);
console.log("keys:", Object.keys(cluster.settings).sort().join(","));
console.log("args array:", Array.isArray(cluster.settings.args));
console.log("exec string:", typeof cluster.settings.exec);
console.log("execArgv array:", Array.isArray(cluster.settings.execArgv));
console.log("silent:", cluster.settings.silent);
