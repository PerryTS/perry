import { SourceLike } from "./source.ts";
export class TextureLike {
  source = new SourceLike();
  set needsUpdate(value: boolean) { this.source.needsUpdate = value; }
}
