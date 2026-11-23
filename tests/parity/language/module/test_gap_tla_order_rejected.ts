import("./fixtures/tla_order/rejected/graph.ts").then(
  () => console.log("unexpected success"),
  error => console.log("rejected", error.message),
);
console.log("driver");
