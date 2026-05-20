const u = new URL("https://example.com/p");
u.protocol = "http:";
console.log("protocol changed:", u.protocol, u.href);

u.hostname = "other.test";
console.log("hostname changed:", u.hostname, u.host);

u.port = "9000";
console.log("port set:", u.port, u.host);

u.username = "user";
u.password = "pw";
console.log("auth:", u.username, u.password);
console.log("href with auth:", u.href);
