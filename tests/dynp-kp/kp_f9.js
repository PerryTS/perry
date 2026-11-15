class A{static x=1; static get g(){return "g"+this.name} static m(){return 2}}
class B extends A{static y=3}
const L=[A,B];
for(let r=0;r<4;r++){const c=L[r&1]; console.log(r, c.prototype.constructor===c, c.x, c.y, c.g, c.m(), Object.getPrototypeOf(c)===(r&1?A:Function.prototype), typeof c.prototype, c.name, c.length)}
B.x=9; console.log(L[1].x, L[0].x, L[1].prototype instanceof A);
