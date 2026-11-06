import { format } from "node:url";

console.log("format:", format({ protocol: "https:", hostname: "example.com", port: "8080", pathname: "/x", query: { a: "1" } }));
