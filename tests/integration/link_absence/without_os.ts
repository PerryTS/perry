console.log(process.argv.slice(2).join(","));
console.log(typeof process.pid,typeof process.env.PATH);
process.nextTick(()=>{console.log("tick");Promise.resolve(42).then(value=>console.log(value))});
