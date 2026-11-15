class K{ static #p=1; #q=2; static s=3; static getp(){return K.#p} }
class E extends K{}
class M extends Map{}
function* g(){} g.prototype={z:1};
const keys=["#p","#q","#<perry:prototype>","__perry_parent_class","__perry_ctor_class_object","__perry_ctor_caps","#<perry:native-payload-js-state>","__perry_collection_backing__","__perry_promise_backing__","#<perry:class-evaluation-prototype>","#<perry:private-class-lexical-binding>","[[GeneratorPrototype]]"];
const R=[K,g,E,M,function(){}];
for(let i=0;i<R.length;i++){const r=R[i]; for(const k of keys) if(r[k]!==undefined) console.log("LEAK", i, k);
  console.log(i, typeof r.__perry_parent_class, typeof r.__perry_ctor_class_object, typeof r.__perry_ctor_caps, typeof r.__perry_collection_backing__, Object.getOwnPropertyNames(r).sort().join("|"));
  for(const n of Object.getOwnPropertyNames(r)) console.log(" ", n, typeof r[n]);}
console.log(K.getp(), Object.getPrototypeOf(g())===g.prototype, g.prototype.z, new M([[1,2]]).get(1));
