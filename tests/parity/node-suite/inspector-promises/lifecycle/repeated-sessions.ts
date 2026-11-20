import { Session } from "node:inspector/promises";

const first = new Session();
const second = new Session();
first.connect();
second.connect();
try {
  const [one, two] = await Promise.all([
    first.post("Runtime.evaluate", { expression: "20 + 1" }),
    second.post("Runtime.evaluate", { expression: "20 + 2" }),
  ]);
  console.log("values:", one.result.value, two.result.value);
} finally {
  first.disconnect();
  second.disconnect();
}
