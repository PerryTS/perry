class A { x = (() => { console.log("BODY:inline"); return 1; })(); }
if (process.argv[2] === "first") { console.log(new A().x); }
else { console.log(new A().x); }
