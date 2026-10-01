Update the SWC ECMAScript loader to 27.0.0 together with its compatible bundler,
parser, TypeScript and React transforms in the runtime TypeScript extension.
This fixes the loader Resolution/SourceFile type mismatches caused by mixing
SWC generations. The compiler parser and HIR retain their compatible SWC stack;
the extension boundary exchanges strings and JavaScript values rather than ASTs.

Add a two-module TypeScript bundling regression that resolves an imported module,
erases its types, and verifies the emitted JavaScript contains the dependency
body and entry export without a remaining import.
