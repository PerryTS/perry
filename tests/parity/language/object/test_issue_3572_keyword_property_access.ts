const obj: any = { await: 0, yield: 1, static: 2, implements: 3 };

obj.await = "await";
obj.yield = "yield";
obj.static = "static";
obj.implements = "implements";

console.log("keyword-props", obj.await, obj.yield, obj.static, obj.implements);
