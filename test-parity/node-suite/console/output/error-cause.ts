const err: any = new Error("outer", { cause: new TypeError("inner") });
err.code = "E_OUTER";
console.log("error cause:", err);
console.error("error stderr:", err.name, err.message, err.code);
