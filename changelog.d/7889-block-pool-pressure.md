Critical memory pressure now drains recycled arena blocks after the owed full
collection, and the recycled-block allowance is shared process-wide and scales
with constrained-device heap budgets.
