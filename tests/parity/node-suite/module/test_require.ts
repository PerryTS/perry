// Test CommonJS require() support for built-in modules

// Test require('fs')
const myFs = require('fs');

// Test require('path')
const myPath = require('path');

// Test require('crypto')
const myCrypto = require('crypto');

// Test fs module through require alias
myFs.writeFileSync("/tmp/compilets_require_test.txt", "Hello from require()!");
let exists = myFs.existsSync("/tmp/compilets_require_test.txt");
console.log(exists); // Should print 1

let content = myFs.readFileSync("/tmp/compilets_require_test.txt");
console.log(content); // Should print "Hello from require()!"

myFs.unlinkSync("/tmp/compilets_require_test.txt");

// Test path module through require alias
let joined = myPath.join("/home/user", "documents/file.txt");
console.log(joined);

let dir = myPath.dirname("/home/user/documents/file.txt");
console.log(dir);

let base = myPath.basename("/home/user/documents/file.txt");
console.log(base);

// Test crypto module through require alias
let uuid = myCrypto.randomUUID();
console.log(uuid);

console.log(99); // Done marker
