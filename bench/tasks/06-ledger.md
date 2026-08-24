# Task: ledger

Define a transaction type with three cases: a deposit carrying an amount, a
withdrawal carrying an amount, and a balance check carrying nothing.

Starting from a balance of 100, apply this sequence in order:

`Deposit(50)`, `Withdraw(30)`, `Check`, `Withdraw(500)`, `Check`,
`Deposit(5)`, `Check`

- a deposit adds to the balance
- a withdrawal subtracts, but only if the balance is at least the amount;
  otherwise the balance is unchanged and the line `rejected: <amount>` is
  printed
- a check prints `balance: <balance>`

## Expected output, exactly

```
balance: 120
rejected: 500
balance: 120
balance: 125
```
