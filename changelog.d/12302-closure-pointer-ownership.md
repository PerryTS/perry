Closure and callable-function predicates now prove that a candidate address
belongs to readable GC storage before inspecting its payload or GC header.
This prevents unmapped heap-range values and small native, proxy, or stream
handles from faulting during closure checks.
