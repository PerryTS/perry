// Natural exit with a program that never names process: everything written
// to a redirected (fully buffered) stdout must be there, in order.
for (let i = 0; i < 20000; i++) console.log("line " + i);
