import { DataTextureLike } from "./fixtures/parity_5891/native_class/data_texture.ts";
function hasOwn(value: any, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
}
const texture: any = new DataTextureLike();
if (texture.dataTag !== "data-texture" || texture.source.version !== 0) throw new Error("initial state");
if (texture.needsUpdate !== undefined) throw new Error("setter-only read");
texture.needsUpdate = true;
texture.needsUpdate = false;
texture.needsUpdate = true;
if (texture.source.version !== 2) throw new Error("version: " + texture.source.version);
if (hasOwn(texture, "needsUpdate") || hasOwn(texture, "isDataTexture")) throw new Error("type-only/setting field leaked");
if (hasOwn(DataTextureLike, "DEFAULT_IMAGE")) throw new Error("declare static leaked");
console.log("OK");
