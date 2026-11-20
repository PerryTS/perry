### fix(codegen): preserve aliased native constructor proofs

Native classes imported under a local alias now retain their canonical
constructor proof through codegen. Reading `net.Socket` methods as values from
an aliased instance therefore returns bound functions, matching the unaliased
import. The proof remains gated to unambiguous native class entries in the API
manifest. Fixes #8222.
