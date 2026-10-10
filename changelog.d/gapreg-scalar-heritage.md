A class declared inside a function with a member-expression heritage over a
local object literal (`const mod = { Base: First }; class M extends mod.Base {}`)
no longer throws "Class extends value is not a constructor". Scalar
replacement of the literal skipped the heritage read when it collected the
fields to keep, so the read folded to `undefined`.
