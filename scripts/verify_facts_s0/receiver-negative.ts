class Box { v = 1; }
function read(b: any) { return b.v; }
console.log(read(new Box()));
console.log(read(JSON.parse("{\"v\":\"x\"}")));
