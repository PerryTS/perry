import { createTracing } from "node:trace_events";

const inherited = createTracing(Object.create({ categories: ["inherited"] }));
console.log("inherited:", inherited.categories);

let reads = 0;
const accessorOptions = {
  get categories() {
    reads++;
    return ["accessor"];
  },
};
const accessor = createTracing(accessorOptions);
console.log("accessor:", accessor.categories, "read:", reads > 0);

const original = ["original"];
const options = { categories: original };
const tracing = createTracing(options);
options.categories = ["replacement"];
console.log("options replacement:", tracing.categories);
