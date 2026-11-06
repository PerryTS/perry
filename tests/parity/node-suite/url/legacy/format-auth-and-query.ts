import url from "node:url";

console.log("format object:", url.format({ protocol: "http", slashes: true, auth: "u:p", hostname: "example.com", pathname: "/a b", query: { x: "1 2" } } as any));
console.log("format url:", url.format(new URL("https://example.com/a?b=c")));
try { console.log("format bad:", url.format(123 as any)); } catch (err: any) { console.log("format bad:", err?.name, err?.code || "no-code"); }
