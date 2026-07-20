// #6666: node v26 coerces a string exitCode to a number at assignment time
// (`process.exitCode = "2"` reads back as the number 2), and natural exit
// uses the coerced integer (node rc=2).
process.exitCode = "2";
console.log("type:", typeof process.exitCode, "value:", process.exitCode);
