// process.title is settable and reads back the assigned value.
process.title = "perry-test-title";
console.log("round-trip:", process.title === "perry-test-title");
