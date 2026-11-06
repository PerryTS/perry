// If/else, while, for, switch
if (true) { console.log("if-true"); }
if (false) { console.log("bad"); } else { console.log("else"); }

let i = 0;
while (i < 3) { i = i + 1; }
console.log(i);

let sum = 0;
for (let j = 1; j <= 5; j = j + 1) { sum = sum + j; }
console.log(sum);

const val = "b";
switch (val) {
  case "a": console.log("case-a"); break;
  case "b": console.log("case-b"); break;
  default: console.log("default"); break;
}
