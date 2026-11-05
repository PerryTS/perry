// Test string slice method
let hello: string = "Hello World";
let sub: string = hello.slice(0, 5);
console.log(sub);  // Should print "Hello"
console.log(sub.length);  // Should print 5

let mid: string = hello.slice(6, 11);
console.log(mid);  // Should print "World"

console.log("String slice tests passed!");
