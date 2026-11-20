// HWS hook: js_runtime_install_bigint
// HWS prefixes: num_bigint::|perry_runtime::bigint::|js_bigint_
// HWS companion: bigint
// HWS argv: ["bigint"]
import {generatePrimeSync} from "node:crypto";
const options:any={}; options[process.argv[2]]=true;
const n:any=generatePrimeSync(8,options);
console.log(typeof n,n>=128 && n<256);
