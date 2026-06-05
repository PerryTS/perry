function stringTag(): First {
  return First.B;
}

function numericTag(): string {
  return Numbered[Numbered.C];
}

enum First {
  A = "A",
  B = "B",
  C = "C",
}

enum Numbered {
  A,
  B,
  C,
}

console.log("string fwd:", stringTag());
console.log("numeric fwd:", numericTag());
console.log("direct:", First.C, Numbered.B);
