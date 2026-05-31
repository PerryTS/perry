// Gap test: WHATWG IPv4 / numeric host canonicalization for node:url
// (#3056, #3059).
//
// #3056: `url.hostname = value` Web-IDL-stringifies the RHS and runs the
//   WHATWG host parser, so a numeric / IPv4-shorthand host canonicalizes to a
//   dotted-quad IPv4 address (`123` -> "0.0.0.123", "0x7f.1" -> "127.0.0.1").
//   Ordinary + IDN hostnames are unchanged; an out-of-range numeric host
//   (`999999999999`) leaves the existing hostname untouched.
// #3059: `url.domainToASCII` / `url.domainToUnicode` apply the same WHATWG
//   host parsing to their (stringified) argument: numeric -> IPv4, IDN ->
//   punycode / Unicode, invalid -> "".
//
// Compared byte-for-byte against `node --experimental-strip-types`.

import url from "node:url";

const u1 = new URL("http://x/");
u1.hostname = 123 as any;
console.log("hostname number:", u1.hostname);
console.log("host number:", u1.host);
console.log("href number:", u1.href);

const u2 = new URL("http://x/");
u2.hostname = "0x7f.1";
console.log("hostname hex:", u2.hostname);

const u3 = new URL("http://x/");
u3.hostname = "example.com";
console.log("hostname normal:", u3.hostname);

const u4 = new URL("http://x/");
u4.hostname = "münchen.de";
console.log("hostname idn:", u4.hostname);

const u5 = new URL("http://x/");
u5.hostname = "999999999999";
console.log("hostname oob (unchanged):", u5.hostname);

console.log("domainToASCII number:", url.domainToASCII(123 as any));
console.log("domainToUnicode number:", url.domainToUnicode(123 as any));
console.log("domainToASCII idn:", url.domainToASCII("münchen.de"));
console.log("domainToUnicode puny:", url.domainToUnicode("xn--mnchen-3ya.de"));
console.log("domainToASCII normal:", url.domainToASCII("example.com"));
console.log("domainToASCII oob:", JSON.stringify(url.domainToASCII("999999999999")));
