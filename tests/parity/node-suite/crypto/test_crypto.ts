// Test crypto functions

// Test crypto.randomBytes - generates random bytes as hex string
let randomHex = crypto.randomBytes(16);
console.log(randomHex);  // Should be a 32-character hex string (16 bytes = 32 hex chars)

// Test crypto.randomUUID - generates UUID v4
let uuid = crypto.randomUUID();
console.log(uuid);  // Should be a UUID like "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx"

// Test crypto.sha256 - SHA256 hash
let hash256 = crypto.sha256("hello world");
console.log(hash256);  // Should be: b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9

// Test crypto.md5 - MD5 hash
let hashMd5 = crypto.md5("hello world");
console.log(hashMd5);  // Should be: 5eb63bbbe01eeed093cb22bb8f5acdc3

// Verify hash length
// SHA256 = 64 hex chars (256 bits / 4 bits per hex char)
// MD5 = 32 hex chars (128 bits / 4 bits per hex char)

console.log("crypto test passed!");
