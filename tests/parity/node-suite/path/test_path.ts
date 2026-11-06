// Test path module

// Test path.join
let joined = path.join("/home/user", "documents/file.txt");
console.log(joined);

// Test path.dirname
let dir = path.dirname("/home/user/documents/file.txt");
console.log(dir);

// Test path.basename
let base = path.basename("/home/user/documents/file.txt");
console.log(base);

// Test path.extname
let ext = path.extname("/home/user/documents/file.txt");
console.log(ext);

// Test path.resolve with relative path
let resolved = path.resolve(".");
console.log(resolved);

console.log(99); // Done marker
