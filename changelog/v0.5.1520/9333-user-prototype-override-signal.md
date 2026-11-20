Internal prototype wiring no longer masquerades as a user
`Object.setPrototypeOf` override. Object metadata now keeps the conservative
prototype-divergence signal used by cache and shape invalidation separate from
the user-origin signal consumed by property and method dispatch.

User-facing `Object.setPrototypeOf`, object-literal `__proto__`, and
recorded `Object.create` paths publish both signals. Runtime wiring through the
loud prototype recorder retains its existing invalidation behavior without
sending those objects through user-override dispatch.
