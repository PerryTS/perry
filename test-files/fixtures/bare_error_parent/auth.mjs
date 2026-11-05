// opencode's llm/src/route/auth.ts shape: `Error` here is the global.
export class MissingCredentialError extends Error {
  _tag = "MissingCredentialError";
  constructor(source) {
    super(`Missing auth credential: ${source}`);
    this.source = source;
  }
}
