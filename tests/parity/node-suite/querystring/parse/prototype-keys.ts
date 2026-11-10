import querystring from "node:querystring";

const out: any = querystring.parse("__proto__=x&constructor=y&prototype=z&toString=q");
console.log("keys:", Object.keys(out).sort().join(","));
console.log("proto value:", out.__proto__);
console.log("constructor value:", out.constructor);
console.log("plain polluted:", ({} as any).polluted === true);
