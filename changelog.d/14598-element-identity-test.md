Tests: `product_element_identity_preserves_codegen_facts` allows element
source identity to add the two facts keyed by `element_offset`
(`number_element_reads`, `primitive_elements`) and still requires every other
elision fact, the solver sites and stats, and the facts digest to be
unchanged. It had failed on `main` since #14354.
