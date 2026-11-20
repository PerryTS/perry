import { error } from "./fixtures/tla_order/finish-error/error.ts";
const first = import("./fixtures/tla_order/finish-error/graph.ts");
const second = import("./fixtures/tla_order/finish-error/graph.ts");
first.catch(value => console.log("first", value === error));
second.catch(value => console.log("second", value === error));
console.log("driver");
