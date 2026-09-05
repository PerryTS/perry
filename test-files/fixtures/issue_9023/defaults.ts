import ImportedToken from "./token.ts";

export function readDefault(value = new ImportedToken()) { return value.read(); }
export function readThunk(make = () => new ImportedToken()) { return make().read(); }
export class Builder {
 make() { return new ImportedToken(); }
 read(value = new ImportedToken()) { return value.read(); }
}
