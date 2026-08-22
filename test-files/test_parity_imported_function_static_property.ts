import { StripeResource } from "./fixtures/parity_5891/imported_function_static/res.ts";
if (typeof (StripeResource as any).method !== "function") throw new Error("method");
if (typeof (StripeResource as any).extend !== "function") throw new Error("extend");
if ((StripeResource as any).MAX !== 42) throw new Error("MAX");
if ((StripeResource as any).name !== "StripeResource") throw new Error("name");
const method: any = (StripeResource as any).method;
const extend: any = (StripeResource as any).extend;
if (method() !== "M" || extend() !== "E") throw new Error("static function call");
console.log("OK");
