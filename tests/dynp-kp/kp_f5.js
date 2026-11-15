const o={m(){}, get g(){return 1}};
const fs=[()=>1, o.m, async function(){}, Object.getOwnPropertyDescriptor(o,"g").get, Math.max, Object.prototype.hasOwnProperty, Function.prototype, function*(){}, async function*(){}];
for(let r=0;r<2;r++) for(let i=0;i<fs.length;i++){const f=fs[i]; console.log(r, i, typeof f.prototype, f.hasOwnProperty("prototype"), f.call===Function.prototype.call, f.bind===Function.prototype.bind, f.apply===Function.prototype.apply, f.length)}
