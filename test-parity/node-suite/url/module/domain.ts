import { domainToASCII, domainToUnicode } from "node:url";

console.log("ascii:", domainToASCII("bücher.example"));
console.log("unicode:", domainToUnicode("xn--bcher-kva.example"));
console.log("plain:", domainToASCII("example.com"));
