The whole-program facts now publish one record per computed member access
(`recv[key]` read, write and call): the proven receiver set in typed form, its
primitive-kind mask, whether it is unknown (top), and the key class (number
only, a named atom, or any string). The record has its own source identity, so
no verdict site is added and emitted code is unchanged. It is the input for
the realm reach to decide which built-in members a computed access can reach.
