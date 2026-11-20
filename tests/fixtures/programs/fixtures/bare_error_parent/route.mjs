// Another module constructs the imported class.
import { MissingCredentialError } from "./auth.mjs";

export const optional = (source) => new MissingCredentialError(source);
