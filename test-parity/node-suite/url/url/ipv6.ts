const u = new URL("http://[::1]:8080/path?q=1");
console.log("href:", u.href);
console.log("host:", u.host);
console.log("hostname:", u.hostname);
console.log("port:", u.port);

const u2 = new URL("http://[2001:db8::1]/");
console.log("v6 hostname:", u2.hostname);
console.log("v6 host:", u2.host);
console.log("v6 origin:", u2.origin);
