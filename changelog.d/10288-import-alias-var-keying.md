Key the imported-variable set by the importing module local name rather than
the origin module export name. Codegen consults that set to decide whether an
identifier is a variable read or a direct call, and looks it up with the name an
`ExternFuncRef` carries, which in the aliased case is the local name. Recording
the origin export name as well let one import claim an identifier the importing
module binds to something else, so a call through that identifier compiled as a
read of a slot that is not published until the other module initializes:
undefined during module initialization, correct afterwards. Minifiers reuse
short aliases across import statements, so the shape is routine in published
bundles. The sibling map for imported functions was already keyed this way.
