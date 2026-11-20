const err = new Error("boom");
console.log("error fields:", { name: err.name, message: err.message });
