function* makeGen() { yield 1; yield 2; yield 3; }
function fresh(kind: string): any {
  if (kind === "array") return [1,2,3].values();
  if (kind === "typed") return new Uint8Array([1,2,3]).values();
  if (kind === "map") return new Map([[1,1],[2,2],[3,3]]).values();
  if (kind === "set") return new Set([1,2,3]).values();
  if (kind === "string") return "a😀b"[Symbol.iterator]();
  if (kind === "segments") return new Intl.Segmenter().segment("a😀b")[Symbol.iterator]();
  if (kind === "generator") return makeGen();
  let n = 0;
  return {[Symbol.iterator]() { return this; }, next() { return {value:++n,done:n>3}; }};
}
function show(x: any): string { return typeof x === "object" ? x.segment : String(x); }
const kinds = ["typed","array","map","set","string","segments","generator","custom"];
for (const kind of kinds) {
  const it: any = fresh(kind);
  let text = "";
  for (const x of it) { text += show(x); if (text.length === 1) continue; }
  const restIt: any = fresh(kind);
  const [a,...rest] = restIt;
  console.log(kind,"all",text,show(a),rest.map(show).join("|"));
  let x: any, tail: any;
  [x,...tail] = fresh(kind);
  console.log(kind,"assign",show(x),tail.map(show).join("|"));
  const patch: any = fresh(kind); let calls = 0, total = "";
  const original = patch.next;
  patch.next = function() { calls++; return original.call(this); };
  for (const x of patch) { total += show(x); patch.next = original; }
  console.log(kind,"saved-override",calls,total);
}
for (const kind of ["typed","array","map","set","string","segments"]) {
  for (const level of [0,1,2]) {
    for (const name of ["next","return"]) {
      const it: any = fresh(kind);
      let target = it;
      for (let i=0;i<level;i++) target = Object.getPrototypeOf(target);
      const before = Object.getOwnPropertyDescriptor(target,name);
      const next = it.next;
      let gets=0,calls=0;
      Object.defineProperty(target,name,{configurable:true,get() {
        gets++;
        return function() { calls++; return name === "next" ? next.call(this) : {done:true}; };
      }});
      let text="";
      for (const x of it) { text+=show(x); break; }
      console.log(kind,level,name,"accessor",gets,calls,text);
      if (before) Object.defineProperty(target,name,before); else delete target[name];
    }
    const it: any = fresh(kind);
    let target = it;
    for (let i=0;i<level;i++) target=Object.getPrototypeOf(target);
    const before = Object.getOwnPropertyDescriptor(target,"return");
    let closes=0;
    for (const x of it) {
      target.return=function(){closes++;return {done:true};};
      break;
    }
    console.log(kind,level,"return-mid-loop",closes);
    if(before) Object.defineProperty(target,"return",before); else delete target.return;
  }
}
// A hole executes IteratorStep but never IteratorValue.
let events="";
function custom():any {
  let n=0;
  return {[Symbol.iterator]() { return this; }, next() {
    events+="n";n++;
    return {get done() {events+="d";return n>3;},
            get value() {events+="v";return n;}};
  }, return(){events+="r";return {done:true};}};
}
const [,first] = custom();
console.log("holes-binding",first,events);
events="";let second:any;
[,second]=custom();
console.log("holes-assignment",second,events);
events="";const [, ...tail] = custom();
console.log("holes-rest",tail.join("|"),events);
const mutable:any = [undefined,2,3].values();let patches=0;
function patchNext():number {mutable.next=function(){patches++;return {done:true};};return 9;}
const [def=patchNext(),follow,...last] = mutable;
console.log("default-patches-next",def,follow,last.join("|"),patches);
for (const kind of ["typed","array","map","set","string","segments"]) {
  const it:any=fresh(kind);
  const first=it.next(), second=it.next();
  first.value="kept";first.extra=17;
  let count=0;for(const x of it)count++;
  console.log(kind,"manual-retained",first!==second,first.value,first.extra,count);
}
const m:any=new Map([[1,10],[2,20],[3,30]]);let mapText="";
for(const x of m.values()){mapText+=x+"|";if(x===10){m.delete(1);m.set(4,40);}}
console.log("map-delete-append",mapText);
const s:any=new Set([1,2,3]);let setText="";
for(const x of s.values()){setText+=x+"|";if(x===1){s.delete(1);s.add(4);}}
console.log("set-delete-append",setText);
// Typed/direct compiler arms must preserve ordinary close behavior too.
function closeDirect(kind:string) {
 const iterable:any=kind==="array"?[1,2,3]:kind==="map"?new Map([[1,2],[3,4]]):
   kind==="set"?new Set([1,2,3]):kind==="string"?"abc":new Intl.Segmenter().segment("abc");
 const probe:any=iterable[Symbol.iterator]();
 const family=Object.getPrototypeOf(probe);
 const before=Object.getOwnPropertyDescriptor(family,"return");
 let closes=0;
 family.return=function(){closes++;return {done:true};};
 for(const x of iterable)break;
 console.log(kind,"direct-close",closes);
 if(before)Object.defineProperty(family,"return",before);else delete family.return;
}
for(const kind of ["array","map","set","string","segments"])closeDirect(kind);


// Renaming methods cannot turn an ordinary store into a native next lane.
const rename:any=[1,2].values();
const renameFamily=Object.getPrototypeOf(rename);
const nextDesc=Object.getOwnPropertyDescriptor(renameFamily,"next")!;
const tagDesc=Object.getOwnPropertyDescriptor(renameFamily,Symbol.toStringTag)!;
delete renameFamily.next;
renameFamily.return=nextDesc.value;
let renameError="";
try { for(const x of rename)console.log("unreachable",x); }
catch(e:any){renameError=e.name;}
console.log("renamed-next",renameError);
delete renameFamily.return;
Object.defineProperty(renameFamily,"next",nextDesc);
delete renameFamily[Symbol.toStringTag];
let renamedCloses=0;
renameFamily.return=function(){renamedCloses++;return {done:true};};
for(const x of [1,2].values())break;
console.log("replaced-tag-with-return",renamedCloses);
delete renameFamily.return;
Object.defineProperty(renameFamily,Symbol.toStringTag,tagDesc);

for(const bad of [undefined,null,17,{}]){
 const it:any={[Symbol.iterator](){return this;},next:bad,return(){console.log("unexpected-close");return {done:true};}};
 try{for(const x of it)console.log("unexpected-body");}
 catch(e:any){console.log("noncallable-next",e.name);}
}
const proxyIt:any=[1,2].values();
let applyCalls=0;
const originalNext=proxyIt.next;
proxyIt.next=new Proxy(originalNext,{apply(target,receiver,args){
 applyCalls++;return Reflect.apply(target,receiver,args);
}});
let proxyText="";for(const x of proxyIt)proxyText+=x;
console.log("callable-proxy-next",proxyText,applyCalls);

const boundLeft:any=[1,2,3].values(), boundRight:any=[7,8,9].values();
boundLeft.next=boundLeft.next.bind(boundRight);
let boundText="";
for(const x of boundLeft){boundText+=x;delete boundLeft.next;}
console.log("saved-bound-next",boundText);
