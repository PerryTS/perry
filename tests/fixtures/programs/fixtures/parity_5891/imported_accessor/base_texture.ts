export const LinearFilter = 1006;
export class BaseTexture {
  public source: any;
  public version = 0;
  public magFilter: number;
  public minFilter: number;
  public generateMipmaps = true;
  public flipY = true;
  public unpackAlignment = 4;
  constructor(
    image: any = null, _mapping?: any, _wrapS?: any, _wrapT?: any,
    magFilter: number = LinearFilter, minFilter: number = LinearFilter,
    _format?: any, _type?: any, _anisotropy?: any, _colorSpace?: any,
  ) {
    this.source = { data: image, version: 0 };
    this.magFilter = magFilter;
    this.minFilter = minFilter;
  }
  get image(): any { return this.source.data; }
  set image(value: any) { this.source.data = value; }
  get readOnlyTag(): string { return "base-read-only"; }
  set writeOnlyTag(value: string) { this.source.writeOnlyTag = value; }
  set needsUpdate(value: boolean) {
    if (value === true) { this.version++; this.source.version++; }
  }
}
export class AliasAccessorBase {
  public marker: any = "unset";
  get aliasedValue(): any { return this.marker; }
  set aliasedValue(value: any) { this.marker = value; }
}
