The http, net, streams and ws ext-crate tests root values through
`RuntimeHandleScope`. The runtime does not export the shadow-stack entry
points on native stack-map targets, so these tests no longer compiled.
