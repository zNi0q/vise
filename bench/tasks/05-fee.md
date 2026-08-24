# Task: fee

Write a fee function that takes a positive amount and returns that amount
divided by 50. It must never be called with a non-positive amount.

For each amount in `[500, 1000, -20]`, print the amount and its fee, or the
amount and `rejected` when the amount is not positive.

## Expected output, exactly

```
500 -> 10
1000 -> 20
-20 -> rejected
```
