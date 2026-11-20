class A { m() { console.log("BODY:copy"); } }
if (process.argv[2] === "first") { new A().m(); } else { new A().m(); }
