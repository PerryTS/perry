// Test fs module

// Write a file
fs.writeFileSync("/tmp/compilets_test.txt", "Hello from Perry!");

// Check if file exists
let exists = fs.existsSync("/tmp/compilets_test.txt");
console.log(exists); // Should print 1

// Read the file back
let content = fs.readFileSync("/tmp/compilets_test.txt");
console.log(content); // Should print "Hello from Perry!"

// Delete the file
fs.unlinkSync("/tmp/compilets_test.txt");

// Check if it was deleted
let existsAfter = fs.existsSync("/tmp/compilets_test.txt");
console.log(existsAfter); // Should print 0

console.log(99); // Done marker
