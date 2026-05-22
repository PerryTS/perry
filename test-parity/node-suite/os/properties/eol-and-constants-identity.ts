import os from "node:os";

console.log("EOL len:", os.EOL.length > 0);
console.log("constants identity:", os.constants === os.constants);
console.log("signals identity:", os.constants.signals === os.constants.signals);
