Retire a shape-table keys address together with its young-log entry once the
address leaves the GC heap (its Eden block released, or its malloc keys array
freed). The young walk used to drop such an address from the log but keep its
slot index and family. No dead-owner prune could remove them afterwards, and
when the block came back as Eden the stale entries tripped the young log's
rule-2 assertion (#12098). One function now decides whether a drained address
is re-logged, left unlogged because no minor can act on it, or retired with
its entries; the minor walk and the full walk both go through it. A tracked
malloc keys array now stays logged until a minor's sweep frees it.
