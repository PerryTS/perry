import * as statically from "./fixtures/BRQ-esm-source.ts";
async function check() {
  const dynamically = await import("./fixtures/BRQ-esm-source.ts");
  console.log(statically === dynamically);
  console.log(statically === statically);
  console.log(statically.value, dynamically.value);
}
check();
