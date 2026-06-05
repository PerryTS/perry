enum Color {
  Red,
  Green = 4,
  Blue,
}

const c: Color = Color.Blue;
console.log("forward:", Color.Blue, "reverse:", Color[c]);
console.log("red reverse:", Color[0], "green reverse:", Color[4], "missing:", Color[99]);
console.log("string key:", Color["Blue" as any]);

enum Mixed {
  No = 0,
  Yes = "YES",
}

console.log("mixed number:", Mixed.No, Mixed[0]);
console.log("mixed string:", Mixed.Yes, (Mixed as any)["YES"]);
