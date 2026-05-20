import { format } from "node:url";

console.log("with auth:", format({
  protocol: "http:",
  slashes: true,
  auth: "user:pw",
  host: "example.com",
  pathname: "/p",
}));

console.log("no slashes:", format({
  protocol: "http:",
  slashes: false,
  host: "example.com",
  pathname: "/p",
}));

console.log("pathname + hash:", format({
  protocol: "https:",
  hostname: "example.com",
  pathname: "/p",
  hash: "#section",
}));
