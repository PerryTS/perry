import { z } from "./_helpers/issue_7964_top.ts";

console.log(z.NEVER.status);
console.log(typeof z.$brand);
console.log(z.null.test("NULL"));
console.log(z.undefined.test("undefined"));
console.log(z.$constructor());
