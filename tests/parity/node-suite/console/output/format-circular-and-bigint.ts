const circular: any = { name: "root" };
circular.self = circular;
console.log("json circular:%j", circular);
console.log("number bigint:%d", 12n);
console.log("integer bigint:%i", 12n);
console.log("float bigint:%f", 12n);
