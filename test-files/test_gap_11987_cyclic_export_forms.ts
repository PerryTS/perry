// #11826/#11987: exported bindings of an ES module on an import cycle. Each
// form (a literal const, an uninitialized let, destructured consts, a
// function-expression const, a renamed export, namespace reads)
// is in its dead zone before its declarator runs and holds its value after.
import "./_helpers/cyclic_export_forms_11987/a.ts";
