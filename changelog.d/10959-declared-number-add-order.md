Add dedicated regression coverage for #10937: declared-number array elements
and fields on `this` must observe mutations from earlier `valueOf` conversions
in a left-associated addition chain. Right-associated and explicitly
snapshotted controls retain their original values. All six cases assert one
conversion and the resulting mutation, and the fixture asserts its case count.

The production fix already landed in #10921 (`2d370f625`): evaluation-order
faithfulness is checked inside `lower_guarded_numeric_add`, which both the
dynamic and declared-number entries reach. This change preserves that shared
guard and adds no second guard or call-site special case.
