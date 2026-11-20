try {
  require("./fixtures/tla_order/require/m.ts");
} catch (error) {
  console.log(error.code);
}
console.log("entry");
