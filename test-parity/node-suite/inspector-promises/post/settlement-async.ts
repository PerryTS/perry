import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  let settled = false;
  const pending = session.post("Runtime.evaluate", { expression: "42" });
  const observed = pending.then((value) => {
    settled = true;
    return value;
  });
  console.log("same turn:", settled);
  const value = await observed;
  console.log("after await:", settled, value.result.value);
} finally {
  session.disconnect();
}
