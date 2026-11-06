import { Session } from "node:inspector";

const first = new Session();
const second = new Session();
try {
  first.connect();
  second.connect();
  const values: number[] = [];
  await Promise.all(
    [first, second].map((session, index) =>
      new Promise<void>((resolve) => {
        session.post("Runtime.evaluate", {
          expression: `${index + 1} * 10`,
          returnByValue: true,
        }, (err, result) => {
          console.log("callback:", index, err === null, result.result.type);
          values[index] = result.result.value;
          resolve();
        });
      })
    ),
  );
  console.log("values:", values.join(","));
} finally {
  first.disconnect();
  second.disconnect();
}
