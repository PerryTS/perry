// Evaluated after reflect-metadata, before the importing module's body.
const { getMetadata } = Reflect as any;
export const seenAtLoad = [typeof Reflect.getMetadata, typeof getMetadata].join(" ");
