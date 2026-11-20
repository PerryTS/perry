class A { get x() { console.log("BODY:accessor"); return 1; } }
console.log(new A().x);
