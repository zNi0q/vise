# Task: chain

Write three steps that can each fail:

- `parse` fails when its input is negative, with the error `negative`
- `scale` fails when its input is greater than 100, with the error `too large`;
  otherwise it returns the input multiplied by 2
- `finish` fails when its input is zero, with the error `zero`; otherwise it
  returns its input unchanged

A pipeline runs `parse`, then `scale`, then `finish`, stopping at the first
failure and reporting it.

For each input in `[5, -3, 200, 0]`, print `<input>: ok <result>` or
`<input>: failed <error>`.

## Expected output, exactly

```
5: ok 10
-3: failed negative
200: failed too large
0: failed zero
```
