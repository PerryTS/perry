const t: any = setTimeout(() => {}, 1000);
console.log("primitive type:", typeof +t);
console.log("primitive positive:", +t > 0);
clearTimeout(+t as any);
await new Promise(resolve => setTimeout(resolve, 10));
console.log("cleared by primitive");
