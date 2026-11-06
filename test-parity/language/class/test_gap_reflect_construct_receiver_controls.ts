{
class TwoBody { r=1; constructor(){ console.log("body",this.r); } }
class OtherBody { pad=99;r=7; }
const value:any=Reflect.construct(TwoBody,[],OtherBody);
console.log(value.r,value.pad,Object.keys(value).join(","));
}

{
class TwoBound { r=0; constructor(n:number){this.r=n;console.log("body",this.r);} }
class OtherBound { pad=99;r=7; }
const bound:any=TwoBound.bind(null,5);
const value:any=Reflect.construct(bound,[],OtherBound);
console.log(value.r,value.pad,Object.keys(value).join(","));
}

{
class TwoSame { r=1; constructor(){console.log("body",this.r);} }
const value:any=Reflect.construct(TwoSame,[],TwoSame);
const defaulted:any=Reflect.construct(TwoSame,[]);
console.log(value.r,defaulted.r);
}

{
class ReplacementReturn { r=9; }
class TwoReturn { r=1; constructor(){console.log("body",this.r);return new ReplacementReturn() as any;} }
class OtherReturn { pad=99;r=7; }
const value:any=Reflect.construct(TwoReturn,[],OtherReturn);
console.log(value.r,value.pad,Object.keys(value).join(","));
}

{
class TwoNewTarget { r=1; constructor(){console.log(new.target===OtherNewTarget,this.r);} }
class OtherNewTarget { pad=99;r=7; }
const value:any=Reflect.construct(TwoNewTarget,[],OtherNewTarget);
console.log(value.r,value.pad,Object.keys(value).join(","));
}

{
class TwoUnknown { r=1; constructor(){console.log("body",this.r);} }
class OtherUnknown { pad=99;r=7; }
(globalThis as any).__receiverCtor=TwoUnknown;
const target:any=Reflect.get(globalThis,["__receiver","Ctor"].join(""));
const value:any=Reflect.construct(target,[],OtherUnknown);
console.log(value.r,value.pad,Object.keys(value).join(","));
}

{
class TwoUnknownNewTarget { r=1; constructor(){console.log("body",this.r);} }
function OtherUnknownNewTarget(this:any){this.pad=99;this.r=7;}
const value:any=Reflect.construct(TwoUnknownNewTarget,[],OtherUnknownNewTarget);
console.log(value.r,value.pad,Object.keys(value).join(","));
}
