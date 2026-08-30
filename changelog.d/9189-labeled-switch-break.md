Fixed async functions that used `await` inside a labeled `switch` and then
executed `break <label>`; the generated binary could previously spin forever
instead of continuing after the switch.
