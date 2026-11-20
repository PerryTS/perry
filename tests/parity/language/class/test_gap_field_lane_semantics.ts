var out=[];
// delete-then-access under warm read/write plans
function F(){ this.a=1; this.b=2; }
var o=new F();
for(var i=0;i<50000;i++){ o.a=i; var x=o.a; }
delete o.a;
out.push("d1="+(o.a===undefined)+"|"+o.b);
o.a=77; out.push("d2="+o.a);
// inherited getter via class after warm own-reads on another key
class G { constructor(){ this.own=5; } get computed(){ return this.own*2; } }
var g=new G();
for(var i=0;i<50000;i++){ var y=g.own; }
out.push("g1="+g.computed+"|"+g.own);
// getter/setter pair on class, hot loop
class H { constructor(){ this._v=0; } get v(){ return this._v; } set v(x){ this._v=x+1; } }
var h=new H();
for(var i=0;i<20000;i++){ h.v=i; }
out.push("h1="+h.v+"|"+h._v);
console.log(out.join(" "));
