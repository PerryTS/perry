const https443 = new URL("https://example.com:443/p");
console.log("https:443 port:", https443.port);
console.log("https:443 host:", https443.host);
console.log("https:443 href:", https443.href);

const http80 = new URL("http://example.com:80/");
console.log("http:80 port:", http80.port);
console.log("http:80 host:", http80.host);
console.log("http:80 href:", http80.href);

const ws80 = new URL("ws://example.com:80/");
console.log("ws:80 port:", ws80.port);

const explicit = new URL("https://example.com:8443/");
console.log("https:8443 port:", explicit.port);
console.log("https:8443 host:", explicit.host);
