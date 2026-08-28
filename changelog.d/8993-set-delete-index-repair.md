`js_set_delete` repairs the index in place instead of rebuilding it.

Each delete did two O(n) things, making an N-element drain O(N²) twice over: survivors were shifted with a barriered store per element rather than one move (N barrier entries per delete), and `rebuild_set_index` then cleared the lookup table and re-inserted every survivor. The index is now repaired in place.
