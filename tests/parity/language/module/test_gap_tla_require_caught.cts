try {
  require("./fixtures/tla_order/require-caught/bad.cts");
} catch (error) {
  console.log("caught", error.message);
}
import("./fixtures/tla_order/require-caught/m.ts").then(() => console.log("done"));
