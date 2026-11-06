const u = new URL("https://example.com:8080/a?x=1#h");
u.hostname = "mañana.com";
console.log("href:", u.href);
console.log("origin:", u.origin);
console.log("hostname:", u.hostname);
console.log("host:", u.host);
