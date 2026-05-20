import { escape } from "node:querystring";

console.log("latin1:", escape("café"));
console.log("emoji:", escape("🙂"));
console.log("mixed:", escape("a b/café"));
