import { TextureLike } from "./texture.ts";
export class DataTextureLike extends TextureLike {
  declare readonly isDataTexture: true;
  declare static readonly DEFAULT_IMAGE: unknown;
  dataTag = "data-texture";
}
