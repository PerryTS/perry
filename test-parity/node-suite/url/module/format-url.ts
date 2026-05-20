import { format } from "node:url";

console.log("format full:", format(new URL("https://example.com/path?q=1#h")));
console.log("format no search frag:", format(new URL("https://example.com/path?q=1#h"), { search: false, fragment: false }));
