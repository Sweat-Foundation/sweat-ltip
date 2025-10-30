## Contract attributes
cliff duration = 31,556,952 seconds
vesting duration = 94,670,856 seconds

## Terms
The contract operates with token units.
1 token = 1000000000000000000 token units.

## Scenarios

1. Alice has a Grant for 94670856 tokens.
At 1000 seconds after the cliff end she claims.
Her order now must be 1000 tokens.
An Executor terminates her Grant with timestamp `cliff_end - one day`.
The order must be cancelled. The Grant's total amount, grant amount and claimed amount must be 0.

2. Alice has a Grant for 94670856 tokens.
At 1000 seconds after the cliff end she claims.
Her order now must be 1000 tokens.
An Executor buys 100% of the Order.
The Grants claimed amount now 1000 tokens.
1000 seconds later the Executor terminates the Grant.
Now total amount == claimed amount == 1000 tokens. Order amount is 0.

3. Alice has a Grant for 94670856 tokens.
At 1000 seconds after the cliff end she claims.
Her order now must be 1000 tokens.
The Executor terminates the Grant at timestamp 500 seconds after the cliff end.
The order now must be cut to 500 tokens, and the total amount as well.

4. Alice has a Grant for 94670856 tokens.
At 1000 seconds after the cliff end she claims.
Her order now must be 1000 tokens.
The Executor buys 100% of the Grant.
Then the Executor terminates the Grant at timestamp 500 seconds after the cliff end.
The Grant's total amount now must be 1000 tokens.

5. Alice has a Grant for 94670856 tokens.
The Executor terminates the Grant 1000 seconds before the cliff end.
Total amount now must be 0.

6. Alice has a Grant for 94670856 tokens.
The Executor terminates it 5000 seconds after the cliff end.
Now the total amount must be 5000 tokens.
The Executor terminates it once again with timestamp 1000 seconds after the cliff.
Termination call fails.
