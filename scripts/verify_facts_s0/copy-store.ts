class Box { v: any; constructor() { this.v = "s"; } }
if (process.argv[2] === "first") {
  console.log(new Box().v);
} else {
  console.log(new Box().v);
}
