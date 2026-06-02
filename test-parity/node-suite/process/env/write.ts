// Writing to process.env coerces to a string and round-trips; deleting it
// makes the read undefined again.
process.env.PERRY_RT = "hello";
console.log("after set:", process.env.PERRY_RT);
process.env.PERRY_NUM = 42 as any;
console.log("coerced:", process.env.PERRY_NUM, typeof process.env.PERRY_NUM);
delete process.env.PERRY_RT;
console.log("after delete:", process.env.PERRY_RT);
