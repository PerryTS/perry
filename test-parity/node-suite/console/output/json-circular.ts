const circular: any = { name: "circle" };
circular.self = circular;
console.log("circular:", "%j", circular);
