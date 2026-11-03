New `perry compile --module-alias <module>=<file>` resolves a module to a
source file, the job a Bun build plugin does. A specifier key maps a virtual
module to generated source, as onResolve does (OpenCode v2's
`virtual:opencode-app-assets`); a path key (`./`, `../`, `/`) replaces a
module file's source, as onLoad does, and the replacement's imports resolve
from the replaced file's location.
