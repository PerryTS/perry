# Observational facts dump records

`PERRY_FACTS_DUMP=<path>` additionally captures JSON records prefixed by `D `.
There is no additional environment switch. With no dump requested, the capture
state is absent. These records never participate in a verdict, a contract,
a module digest, a cache key, or code generation. The existing M/T/F/FACT/S
records retain their format and meaning.

Every D record names its emitting analysis `unit`. In whole-program mode this
is `<whole>`; in unit mode it is the driver's actual solve key. All node,
token, function and layout IDs are local to that emitting unit. A source
identity is a module path and SWC's 1-based byte interval `[start, end)`.
Zero coordinates identify synthesized or transformed HIR without a retained
source coordinate. Namespace routing has an explicit origin tag. A consumer
must account for those records instead of joining them to a nearby expression.
This is an observation of final HIR operations, not a census of every original
AST expression: specialized native operations and erased source expressions
can have no corresponding generic property/index/call/new HIR operation.

- `analysis`: aborted status, deterministic solve work and its unchanged bound.
  An aborted snapshot is partial and cannot establish a fact.
- `site`: property, element, value-call or new operation; source interval;
  containing function; receiver, result, index and callee node IDs where
  available; source call ID; arguments and spreads; raw callee token IDs and
  symbolic builtin summaries. A method call need not materialize a separate
  callee node. Its raw binding observations remain available on failure.
- `verdict`: the legacy site's final verdict, with its emitting unit, including
  post-census guarded verdicts. It is separate from the independent values.
- `node`: primitive and value-kind masks, TOP, unknown-publication markers,
  all token alternatives, parameter/observation flags, and the first-arrival
  TOP root and available origin. Primitive bits are number=1, string=2,
  boolean=4, null=8, undefined=16, bigint=32, symbol=64. Kind masks additionally
  use object=128 and function=256. Internal tracking markers are identified
  separately; an unresolved marker is not a concrete source object.
- `token`: raw token category and identity, layout/function/context IDs,
  marker flags, escape reason, and bound-function metadata. Bound metadata
  includes target and this nodes, the ordered bound argument nodes, and the
  source bind origin when one exists. Follow nested bound targets explicitly.
- `function`: HIR identity, declaration module and source span when retained,
  parameter/this/result nodes and containing function. `declarationKey` is the
  authoritative function-contract identity: another unit's stand-in can be
  resolved against the owner's source record without guessing from a name.
  Generated runtime functions can legitimately have no source declaration.
- `layout`: declaration module/span for source classes, parent, observed parent
  links, complete statically settled ancestry, native parent and template kind.
  Synthetic literal layouts have no class declaration; their display identity
  can still resolve a foreign stand-in against its owning unit's layout.
- `top-field`: existing solved fields holding TOP, including any-key fields.
  It names the holder token, field and value node; inspecting it creates no
  field, edge, node or constraint.

Raw callee observations use original generation-call IDs and summary-template
identities. Allocation-site numbers are not source identities: synthetic
protocol and merged bound calls may use allocation-site zero. Their callees
must not be attributed to source call zero. Bound expansions are represented
by the original bound token and its target metadata, rather than pretending
that an unbound body is the original callable value. Symbolic builtin summary
alternatives are distinct from named compiled-body tokens and from opaque
builtin tokens. An empty raw binding set does not prove runtime unreachability.

TOP roots describe a first-arrival path, not all causal paths or a sound
counterfactual ceiling. Ordinary dump mode records this path without enabling
explain mode or creating explain-only nodes. Shared TOP origins can remain
unnamed; explain mode can supply additional provenance, but is a separate
analysis run and must be checked for equivalent facts before attribution.

An offline checker can resolve a generic signature in the source calling
context, compare raw function declarations structurally, and follow the
recorded bind origins and bound prefixes. Signature compatibility is a check,
not a proof of execution or an input to Perry. Cast/any/non-null syntax flags
and any builtin fixture signatures belong exclusively in that offline tool.


## Publication route observations (PUB)

The same dump capture also observes existing publication state:

- `parameter-marker` names the owning function and parameter index.
- `marker-route` reports existing bare/beside state, members, reach/path parents
  and an existing argument home node. It never creates a member or settles it.
- `parameter-contract` reports existing closedness, described positions,
  incoming value nodes, contract-function membership and foreign stand-in state.
- `contract-input` labels a stand-in value node with its existing input summary.
- `copy-edges` reports existing successor edges; `top-predecessor` reports the
  retained first-arrival path. Empty reason text is allowed without explain.
- `copy-verdict` reports, per site, the verdict of each function copy holding it
  (an inlined body is a copy of its own), keyed by module path, HIR id and name,
  so a dump compares one copy with the same copy of another run.
- `argument-output` and `value-output` report selected output summary fields
  after derivation. These are intentionally partial views of a contract; an
  omitted field is not proof that the contract lacks that kind of value.

All IDs remain local to the emitting unit. Authoritative function declaration
keys and source module/span identify owners across units. These observations
never change contracts, verdicts, digests, solve work or code generation, and
with capture disabled their state is absent. An inter-unit OFF/ON fixture
asserts unchanged bindings, proofs, broken set, rounds, digests and work, and
requires nonempty publication-route records.
