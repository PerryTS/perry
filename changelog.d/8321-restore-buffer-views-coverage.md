### Changed

- **`native_proof_buffer_views` runs in per-PR CI again.** #8302 restored the six
  assertions that #8264 recorded as baseline reds, but their
  `SUITE_EXCLUSIONS` entries in `scripts/ci_e2e_scope.py` stayed behind. A stale
  exclusion is not a red build — it is silence: the suite keeps being skipped,
  so the coverage the fix earned back stays dark, which is #7708's failure mode.

  The six entries are removed and the suite is added to the mapped set (an
  unexcluded suite must be mapped, or `--self-test` refuses it). Verified 45/45
  passing on `main` at the merge of #8302.

  The other two #8264 entries are still genuinely red and keep their exclusions:
  `shadow_slot_hygiene::canonical_str_local_keeps_shadow_binding_and_tag_dispatched_ops`
  and `typed_feedback::typed_feedback_guards_direct_class_field_specialization`
  (re-checked at the same commit: 12/13 and 16/17).
