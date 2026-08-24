# Task: door

A door is in one of three states: `Closed`, `Open`, or `Locked`. Four events can
arrive: `OpenIt`, `CloseIt`, `LockIt`, `UnlockIt`.

Only these transitions are allowed:

- `Closed` + `OpenIt` becomes `Open`
- `Closed` + `LockIt` becomes `Locked`
- `Open` + `CloseIt` becomes `Closed`
- `Locked` + `UnlockIt` becomes `Closed`

Any other combination is invalid: print `invalid: <event> while <state>` and
leave the state unchanged.

After every event, print `state: <state>`.

Starting from `Closed`, apply: `OpenIt`, `LockIt`, `CloseIt`, `LockIt`,
`UnlockIt`, `OpenIt`.

## Expected output, exactly

```
state: Open
invalid: LockIt while Open
state: Open
state: Closed
state: Locked
state: Closed
state: Open
```
