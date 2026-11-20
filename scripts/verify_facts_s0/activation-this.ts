class A { x = (() => { console.log("BODY:this"); return 1; })(); }
console.log(new A().x);
