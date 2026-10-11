Arguments objects now keep their mapped-parameter state in their own allocation,
without an ObjectMeta allocation. Their existing birth shape admits indexed and
length slot reads; structural mutations retain ordinary property semantics.
Dynamic calls to materialized synthetic-arguments bodies create that object
directly from rooted caller values, avoiding an intermediate rest array.

Typed `f.apply(x, arguments)` and `f.call(x, ...arguments)` use the actual method
selected at each call. Intrinsic identity and the live arguments shape allow a
borrowed argument list; replaced methods, mapped parameters, accessors, escaping
objects and custom iterators retain their observable behavior.
