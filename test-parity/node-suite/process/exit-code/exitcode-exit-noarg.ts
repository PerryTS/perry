// #6666: a bare process.exit() (no argument) exits with the stored
// process.exitCode rather than forcing 0 (node rc=3).
process.exitCode = 3;
console.log("x");
process.exit();
