import { AliasAccessorBase, BaseTexture } from "./fixtures/parity_5891/imported_accessor/base_texture.ts";
const NearestFilter = 1003;
function assert(condition: any, label: string): void { if (!condition) throw new Error(label); }

class DataTextureLike extends BaseTexture {
  declare magFilter: number;
  declare minFilter: number;
  declare generateMipmaps: boolean;
  declare flipY: boolean;
  declare unpackAlignment: number;
  public isDataTexture = true;
  constructor(
    data: any = null, width = 1, height = 1, format?: any, type?: any,
    mapping?: any, wrapS?: any, wrapT?: any, magFilter: number = NearestFilter,
    minFilter: number = NearestFilter, anisotropy?: any, colorSpace?: any,
  ) {
    super(null, mapping, wrapS, wrapT, magFilter, minFilter, format, type, anisotropy, colorSpace);
    this.image = { data, width, height };
    this.generateMipmaps = false;
    this.flipY = false;
    this.unpackAlignment = 1;
    this.writeOnlyTag = "ctor-setter";
  }
}

const payload = new Uint8Array(8);
const texture: any = new DataTextureLike(payload, 2, 1);
assert(texture.isDataTexture === true, "subclass field initialized");
assert(texture instanceof BaseTexture, "instanceof imported base");
assert(texture.version === 0, "initial version");
assert(texture.image.data === payload && texture.image.width === 2 && texture.image.height === 1, "image payload");
assert(texture.readOnlyTag === "base-read-only", "getter-only inherited accessor");
assert(texture.source.writeOnlyTag === "ctor-setter", "setter-only inherited accessor");
assert(texture.magFilter === NearestFilter && texture.minFilter === NearestFilter, "filter defaults");
assert(texture.generateMipmaps === false && texture.flipY === false && texture.unpackAlignment === 1, "field overrides");
assert(!Object.keys(texture).includes("image"), "accessor write created image slot");
assert(!Object.keys(texture).includes("readOnlyTag"), "getter created slot");
assert(!Object.keys(texture).includes("writeOnlyTag"), "setter created slot");

class AliasAccessorChild extends AliasAccessorBase {}
const aliasProto: any = AliasAccessorChild.prototype;
aliasProto.aliasedValue = function badAliasPatch() { return "bad"; };
const aliasInstance: any = new AliasAccessorChild();
aliasInstance.aliasedValue = "instance-setter";
assert(aliasInstance.aliasedValue === "instance-setter", "prototype alias write");
assert(!Object.keys(aliasInstance).includes("aliasedValue"), "prototype alias created slot");

class StaticAccessorPollution {
  static get constructorSlot(): string { return "static-slot"; }
  static set constructorSlot(_value: string) {}
  constructor() { (this as any).constructorSlot = "instance-slot"; }
}
const staticPollution: any = new StaticAccessorPollution();
assert(StaticAccessorPollution.constructorSlot === "static-slot", "static accessor");
assert(staticPollution.constructorSlot === "instance-slot", "static accessor suppressed field");
assert(Object.keys(staticPollution).includes("constructorSlot"), "constructor assignment not own");

const replacement = { data: payload, width: 4, height: 3 };
texture.image = replacement;
assert(texture.image === replacement, "inherited setter round-trip");
texture.writeOnlyTag = "post-constructor";
assert(texture.source.writeOnlyTag === "post-constructor", "setter-only post-constructor write");
texture.needsUpdate = true;
assert(texture.version === 1 && texture.source.version === 1, "needsUpdate versions");
console.log("OK");
