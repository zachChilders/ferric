# Task: Stdlib — `math`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `math` module from `docs/stdlib.md` (section "`math` — Numeric
Operations").

Standalone — no shared types beyond `Int` and `Float`.

## Output File

- `crates/ferric_stdlib/src/math.rs`

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

None new. `Int(i64)` and `Float(f64)` already exist.

## Required Deps

None — `f64` math is in std.

## Function Surface (must register)

Integer:

`abs`, `min`, `max`, `clamp`, `pow`, `gcd`, `lcm`, `div_floor`, `div_ceil`,
`rem_euclean`.

Float:

`abs_f`, `min_f`, `max_f`, `clamp_f`, `floor`, `ceil`, `round`, `trunc`,
`fract`, `sqrt`, `cbrt`, `pow_f`, `log`, `ln`, `log2`, `log10`, `exp`,
`sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `hypot`, `is_nan`,
`is_finite`, `is_inf`.

Constants (register as zero-arg native functions returning the value):

`PI`, `E`, `TAU`, `INF`, `NAN`.

Conversion:

`int_to_float`, `float_to_int`.

## Implementation Notes

- `pow(base, exp)` errs on negative `exp` (`MathError::NegativeExponent`
  per spec — represent in the error string).
- `div_floor(a, 0)` and `div_ceil(a, 0)` err
  (`MathError::DivisionByZero`).
- `sqrt(n)` for `n < 0.0` errs (`MathError::NegativeSqrt`).
- `log(n, base)` errs if `n <= 0.0` or `base <= 0.0` or `base == 1.0`.
- `ln`, `log2`, `log10` err for `n <= 0.0`.
- `asin`, `acos` err if `|n| > 1.0`.
- `rem_euclean(a, b)` is non-negative remainder
  (`a.rem_euclid(b)`); err on `b == 0`.
- `gcd(0, 0)` → 0; otherwise standard Euclidean.
- `lcm(0, _)` → 0.
- Constants: register as no-arg functions. The Ferric front end may
  ultimately treat them as constant items — for now expose as nullary
  functions called by name.

## Tests

- `abs(-5)` → 5; `abs(0)` → 0
- `min(3, 7)`, `max(3, 7)`, `clamp(5, 0, 10)`, `clamp(-1, 0, 10)` → 0
- `pow(2, 10)` → 1024; `pow(2, -1)` errs
- `gcd(12, 8)` → 4; `gcd(0, 5)` → 5
- `lcm(4, 6)` → 12; `lcm(0, 5)` → 0
- `div_floor(7, 2)` → 3; `div_floor(-7, 2)` → -4
- `div_ceil(7, 2)` → 4; `div_ceil(-7, 2)` → -3
- `div_floor(_, 0)` errs
- `rem_euclean(-7, 3)` → 2 (non-negative)
- Float roundings: `floor(1.7)` → 1.0; `ceil(1.2)` → 2.0; `round(1.5)` → 2.0
- `sqrt(4.0)` → 2.0; `sqrt(-1.0)` errs
- `cbrt(27.0)` → 3.0
- `log(100.0, 10.0)` → 2.0 (within epsilon)
- `ln(E)` ≈ 1.0
- `log2(8.0)` → 3.0
- `log10(1000.0)` → 3.0
- `exp(0.0)` → 1.0
- `sin(0.0)` → 0.0; `cos(0.0)` → 1.0
- `atan2(1.0, 1.0)` ≈ PI/4
- `hypot(3.0, 4.0)` → 5.0
- `is_nan(NAN())` → true; `is_finite(INF())` → false; `is_inf(INF())` → true
- `int_to_float(5)` → 5.0; `float_to_int(3.9)` → 3 (truncates toward zero)
- `PI()` returns f64::consts::PI

Aim for ~30+ tests.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- Float comparisons use an epsilon tolerance (e.g.
  `(a - b).abs() < 1e-10`).

## Acceptance

- One file with the full math surface.
- Tests cover happy + error paths.
- No edits outside the new file.
