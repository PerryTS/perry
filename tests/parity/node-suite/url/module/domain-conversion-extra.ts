import { domainToASCII, domainToUnicode } from "node:url";

for (const d of ["mañana.com", "☃.net", "xn--maana-pta.com", "bad domain", ""] ) {
  console.log("ascii:", JSON.stringify(d), "=>", domainToASCII(d));
  console.log("unicode:", JSON.stringify(d), "=>", domainToUnicode(d));
}
