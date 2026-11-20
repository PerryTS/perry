// opencode's core/src/ripgrep.ts shape: a module exports its own class named
// `Error` (there made by Effect's Schema.TaggedErrorClass) whose constructor
// validates an options object.
const TaggedError = (tag) =>
  class extends globalThis.Error {
    constructor(props) {
      if (typeof props !== "object" || props === null) {
        throw new TypeError("Expected object, got " + JSON.stringify(props));
      }
      super(props.message);
      this._tag = tag;
    }
  };

export class Error extends TaggedError("Ripgrep.Error") {}
