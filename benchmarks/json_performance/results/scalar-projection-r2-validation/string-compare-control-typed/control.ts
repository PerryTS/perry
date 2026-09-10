let source: string = "";
for (let i = 0; i < 128; i++) source += "record";
function text(n: number): string { return n.toString(); }
console.log("same", text(42) === source);
