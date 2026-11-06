const values = new Array(4);
values[0] = 112;
values[1] = 101;
if (values.length !== 4) throw new Error("expected preserved array length");
if (values[0] !== 112 || values[1] !== 101) throw new Error(`indexed writes: ${values[0]}, ${values[1]}`);
if (values[2] !== undefined) throw new Error("expected unwritten slot to remain undefined");
console.log("new Array index set ok");
