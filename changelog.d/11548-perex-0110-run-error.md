### Changed

- The regex engine moves from perex 0.1.9 to 0.1.10. When a search fails, its run now reports the work it had left, and Perry's search sites record that work instead of keeping the budget they started with. Nothing observable changes today. This prepares the adoption of the perex release that carries PerryTS/perex#3, which cuts the package-shaped regex costs: uuid's `validate` −65%, jws's `JWS_REGEX.test` −84%, jsonwebtoken/decode −52% and uuid/v4 −48% in instructions (#10166).
