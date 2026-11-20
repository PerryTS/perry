// Test Date functionality

// Test Date.now() - static method returning current timestamp
let timestamp = Date.now();
console.log(timestamp);  // Should print a large number (milliseconds since epoch)

// Test new Date() - create date from current time
let now = new Date();
console.log(now.getTime());  // Should print similar timestamp

// Test new Date(timestamp) - create date from specific timestamp
// Using a known timestamp: 2024-01-15 12:30:45.123 UTC = 1705321845123
let specificDate = new Date(1705321845123);
console.log(specificDate.getTime());  // Should print 1705321845123

// Test date component getters
console.log(specificDate.getFullYear());   // Should print 2024
console.log(specificDate.getMonth());       // Should print 0 (January, 0-indexed)
console.log(specificDate.getDate());        // Should print 15
console.log(specificDate.getHours());       // Should print 12
console.log(specificDate.getMinutes());     // Should print 30
console.log(specificDate.getSeconds());     // Should print 45
console.log(specificDate.getMilliseconds()); // Should print 123

// Test toISOString
console.log(specificDate.toISOString());  // Should print "2024-01-15T12:30:45.123Z"

// Test Unix epoch (1970-01-01 00:00:00 UTC)
let epoch = new Date(0);
console.log(epoch.getFullYear());   // Should print 1970
console.log(epoch.getMonth());       // Should print 0 (January)
console.log(epoch.getDate());        // Should print 1
console.log(epoch.getHours());       // Should print 0
console.log(epoch.getMinutes());     // Should print 0
console.log(epoch.getSeconds());     // Should print 0
console.log(epoch.toISOString());    // Should print "1970-01-01T00:00:00.000Z"

// Test that Date.now() returns increasing values
let before = Date.now();
let after = Date.now();
if (after >= before) {
    console.log("Date.now() is monotonically increasing");
}

console.log("Date test passed!");
