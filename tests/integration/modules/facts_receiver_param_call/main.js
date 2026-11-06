import { make } from "./a.js";
console.log(make(function () { this.n = "str"; return 7; }, 2));
