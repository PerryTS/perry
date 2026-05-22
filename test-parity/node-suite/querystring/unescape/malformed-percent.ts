import querystring from "node:querystring";

for (const input of ["%", "%2", "%zz", "%E0%A4%A", "a+b", "a%20b"]) {
  try { console.log("unescape:", input, "=>", querystring.unescape(input)); } catch (err: any) { console.log("unescape:", input, err?.name, err?.code || "no-code"); }
}
