import { Readable } from "node:stream";

async function show(label, promise) {
  try {
    const value = await promise;
    console.log(`${label}: resolved ${String(value)}`);
  } catch (error) {
    console.log(`${label}: threw ${error.name}: ${error.message}`);
    console.log(`${label} code: ${error.code}`);
  }
}

await show(
  "empty no initial",
  Readable.from([]).reduce((acc, value) => acc + value),
);
await show(
  "empty with initial",
  Readable.from([]).reduce((acc, value) => acc + value, 42),
);
await show(
  "nonempty no initial",
  Readable.from([1, 2, 3]).reduce((acc, value) => acc + value),
);
