A closure created before a function-nested class declaration now reads the class
through its lexical binding when the class is lowered per evaluation (heritage
the compiler cannot resolve such as `Array`, `Map` or `Error`, private
elements, or computed field keys). Such a forward reference used to construct
the shared template, so the instance missed the evaluated class's methods and
failed `instanceof` (fuzzysort's `KeysResult extends Array`, which left the
opencode TUI's `/c` slash-command filter with no matches). The binding is
pre-registered as a boxed TDZ local like a forward-captured `let`, and the
declaration initializes it; class bodies resolve their own name as before.
