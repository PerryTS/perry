let source = "[";
for (let i = 0; i < 128; i++) source += "record";
console.log("same", JSON.stringify([]) === source);
