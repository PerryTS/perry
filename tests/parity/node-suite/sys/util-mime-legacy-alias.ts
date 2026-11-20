import * as sys from "node:sys";
import * as util from "node:util";

const mime = new sys.MIMEType("text/plain; charset=utf-8");
const params = new sys.MIMEParams();
params.set("a", "b c");

console.log("constructors:", typeof sys.MIMEType, typeof sys.MIMEParams);
console.log("mime:", mime.essence, mime.params.get("charset"));
console.log("params:", params.toString());
console.log("helpers:", typeof sys._extend, typeof sys._errnoException, typeof sys._exceptionWithHostPort);
console.log("debug-alias:", sys.debug === sys.debuglog, util.debug === sys.debug);
