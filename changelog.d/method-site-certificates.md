Class method dispatch on a receiver of unknown class (`o.step(x)` where several
classes implement `step`) now selects the compiled body from a method
certificate (decision 129, slice 2). The site's record holds, per receiver word
(`class_id | ShapeId`), the body that word resolves the method to, all proven at
one `PERRY_PROTO_VALIDITY` value; the hit is the shared
`PristineReceiverFact::Shape` check (two compares, one load), taken before the
arguments, followed by a direct call. Every miss calls the one generic entry,
`js_method_site_certify`, which re-resolves the method and rewrites the
certificate. The per-call own-property probe, the canonical-shape arms and the
class-directory holder guards of the old dispatch tower are gone (Zod: 29 → 0
own-property probes per parse).

A certificate holds no heap reference (the body info lives in the executable
image). Every prototype on the certified chain is a marked prototype, so a
method replaced in place, a key added or deleted on a prototype, or a relinked
prototype advances the epoch and revokes it. A contract-check
build (`PERRY_CHECK_CONTRACTS=1` while compiling) re-proves each certified hit
on the live receiver and aborts through the runtime's one contract report if it
does not hold; release builds emit nothing for it. The receiver word carries
the full ShapeId, so a receiver with an explicit or foreign prototype (#12466)
never matches a nominal certificate; its certificate names no body and the call
takes the generic method site.
