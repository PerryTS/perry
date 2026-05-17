export const MODULE_METADATA = Object.freeze({
  PROVIDERS: "providers",
  CONTROLLERS: "controllers",
});

export const PATH_METADATA = "path";
export const METHOD_METADATA = "method";
export const PARAMTYPES_METADATA = "design:paramtypes";
export const INJECTABLE_WATERMARK = "__injectable__";
export const CONTROLLER_WATERMARK = "__controller__";
export const RequestMethod = { GET: 0 };

export function Injectable() {
  return target => {
    Reflect.defineMetadata(INJECTABLE_WATERMARK, true, target);
  };
}

export function Controller(prefix) {
  return target => {
    Reflect.defineMetadata(CONTROLLER_WATERMARK, true, target);
    Reflect.defineMetadata(PATH_METADATA, prefix, target);
  };
}

export function Get(path) {
  return (_target, _key, descriptor) => {
    Reflect.defineMetadata(PATH_METADATA, path, descriptor.value);
    Reflect.defineMetadata(METHOD_METADATA, RequestMethod.GET, descriptor.value);
  };
}

export function Module(metadata) {
  return target => {
    for (const property in metadata) {
      if (Object.hasOwnProperty.call(metadata, property)) {
        Reflect.defineMetadata(property, metadata[property], target);
      }
    }
  };
}
