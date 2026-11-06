class A { m() { console.log("BODY:copy"); } }
const a = new A();
if (process.argv[2] === "first") { a.m(); } else { a.m(); }
