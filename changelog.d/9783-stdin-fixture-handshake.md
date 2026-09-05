Make the stdin lifecycle parity fixture wait for each child to finish its toggle
and GC churn before sending the second input chunk. Removing the four fixed
2.5-second waits lets the Node oracle finish within the suite's 10-second budget.
