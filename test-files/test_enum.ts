// Test basic enums

// Numeric enum with auto-increment
enum Color {
    Red,    // 0
    Green,  // 1
    Blue    // 2
}

// Print numeric enum values
console.log(Color.Red);   // Should print 0
console.log(Color.Green); // Should print 1
console.log(Color.Blue);  // Should print 2

// Numeric enum with explicit values
enum Status {
    Pending = 1,
    Active = 5,
    Done = 10
}

console.log(Status.Pending); // Should print 1
console.log(Status.Active);  // Should print 5
console.log(Status.Done);    // Should print 10

// Use enum in comparison
let myColor = Color.Green;
if (myColor === Color.Green) {
    console.log(100); // Should print 100 (means test passed)
}

// Reverse mapping (#4509): numeric enums map value -> name.
const cIdx: Color = Color.Blue;
console.log(Color[cIdx]); // Should print Blue (dynamic index)
console.log(Color[0]);    // Should print Red
console.log(Color[1]);    // Should print Green
console.log(Color[2]);    // Should print Blue
console.log(Status[5]);   // Should print Active
