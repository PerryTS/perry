Speed up declared `Map.get` and `ReadonlyMap.get` calls that pass through
nested interface or object fields. Genuine native Maps now bypass generic
method dispatch, while structural values, subclasses, proxies, primitives,
and nullish receivers retain ordinary JavaScript behavior on a brand miss.
