### Fixed

- **A prototype write on one evaluation of a function-body class expression
  leaked to every other evaluation and never became an own key** (#11134).
  A class expression that lowers to a per-evaluation class object
  (`ClassExprFresh`: statics, captures, private elements, or dynamic heritage)
  gets its own `.prototype` per evaluation, but two write paths keyed the write
  by the shared class TEMPLATE instead:
  - literal `C.prototype.m = v` (and the aliased `const p = C.prototype;
    p.m = v`) lowered to `RegisterPrototypeMethod`, i.e.
    `js_register_prototype_method(template_cid, …)`;
  - computed `C.prototype[k] = v` reached `proxy::target_set`, whose
    declared-prototype branch writes into the same template-keyed method
    registry — and since #11113 (#11043) `class_id_for_decl_prototype_object`
    recognizes per-evaluation prototypes too, so that branch now caught them.

  So every evaluation saw every other evaluation's methods, and
  `Object.keys(C.prototype)` / `hasOwnProperty` did not report them.
  `@redis/client`'s `attachConfig` (`Class.prototype[name] = …` once for the
  client, once for its Multi class) exposed the Multi command stubs on the
  client.

  The HIR now records which class-expression registration keys lowered to
  `ClassExprFresh` (`LoweringContext::fresh_evaluation_classes`) and lets
  prototype writes on them take the ordinary PutValue path onto the evaluated
  `.prototype`; `target_set`'s registry branch now skips per-evaluation
  prototypes (they stay recognized for accessor reflection, so #11043's
  `Object.defineProperties(C.prototype, …)` shape is unchanged). Gap tests:
  `test_gap_11134_class_eval_proto_writes.ts` and
  `test_gap_11042_class_expr_dynamic_heritage_per_evaluation.ts` (the redis
  `attachConfig` shape end to end).
