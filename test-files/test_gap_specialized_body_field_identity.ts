const Layer = { isLayer: (u:any) => u !== null && typeof u === "object" && "~effect/Layer" in u };
class NodeValue<A,T> {
  readonly definition:any; readonly tag:T;
  constructor(definition:any, tag:T) { this.definition=definition; this.tag=tag; }
  get name() { return this.definition.name; }
  replace<Target>(replacement:Target) {
    return makeProvider({name:this.name,layer:replacement,deps:[]},this.tag);
  }
}
class ProviderNode<A,T> extends NodeValue<A,T> {}
function makeProvider<Implementation,Items extends any[],T>(input:{service?:any,name?:string,layer:Implementation,deps:Items},tag:T):ProviderNode<Implementation,T> {
  if (!Layer.isLayer(input.layer)) throw new Error("expected layer");
  return new ProviderNode({name:input.service!==undefined?input.service.key:input.name,implementation:input.layer,dependencies:[...input.deps]},tag);
}
export function tags() {
  const values = {global:"global"};
  return {make:<Name extends string>(name:Name)=><Implementation,Items extends any[]>(input:{service?:any,name?:string,layer:Implementation,deps:Items}) => makeProvider<Implementation,Items,Name>(input,values[name])};
}

const makeGlobalNode = tags().make("global");
const service = {key:"service"};
const layer = Object.create({"~effect/Layer":true});
const a = makeGlobalNode({service,layer,deps:[]});
const b = makeGlobalNode({service,deps:[],layer});
console.log(a.name,b.name);
