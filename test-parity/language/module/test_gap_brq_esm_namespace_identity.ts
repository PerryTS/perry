import * as statically from "../../../test-files/fixtures/BRQ-esm-source.ts";
async function check() {
  const dynamically = await import("../../../test-files/fixtures/BRQ-esm-source.ts");
  console.log(statically === dynamically);
  console.log(statically === statically);
  console.log(statically.value, dynamically.value);
}
check();
