Primitive and typed-array `toLocaleString` calls now throw a `TypeError` when
prototype lookup resolves the invoked method to a non-callable data property or
accessor result, instead of silently falling back to native formatting.
