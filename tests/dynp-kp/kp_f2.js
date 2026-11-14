function F(){} function G(){this.y=2}
const A=[F,G]; const out=[];
for(let r=0;r<4;r++){const f=A[r&1]; out.push(typeof f.prototype, f.prototype.constructor===f, Object.getPrototypeOf(new f())===f.prototype);}
F.prototype={x:1}; for(let r=0;r<2;r++){const f=A[r&2]; out.push(f.prototype.x, Object.getPrototypeOf(new f())===f.prototype);}
F.prototype=3; for(let r=0;r<2;r++){const f=A[r&2]; out.push(f.prototype, Object.getPrototypeOf(new f())===Object.prototype);}
console.log(out.join(","));
