//! # `math` — Numeric Operations
//!
//! Implements the `math` module from `docs/stdlib.md`. All functions are
//! registered as native functions with a `math_` prefix (the spec uses
//! `module::function` in Ferric source; the Rust-side native is
//! `<module>_<function>`).
//!
//! Constants (`PI`, `E`, `TAU`, `INF`, `NAN`) are exposed as zero-arg native
//! functions returning the value. The Ferric front end may eventually expose
//! them as constant items, but for now they are nullary.
//!
//! Errors are encoded as a tagged `Result` value at the Ferric level once
//! algebraic types land; for now, math functions that the spec marks fallible
//! return a Rust `Err(String)` that the VM converts into a `RuntimeError`.

// REQUIRED NATIVE VALUE VARIANTS:
//   None new. `Int(i64)` and `Float(f64)` already exist on `NativeValue`.

// REQUIRED DEPS:
//   None — `f64` math is in std.

use ferric_common::Interner;

use crate::{NativeRegistry, NativeValue};

// ============================================================================
// Local helpers (kept private to this module per overview rule #5)
// ============================================================================

fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!(
            "expected {} argument(s), got {}",
            expected,
            args.len()
        ))
    } else {
        Ok(())
    }
}

fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("expected int, got {other:?}")),
    }
}

fn expect_float(value: &NativeValue) -> Result<f64, String> {
    match value {
        NativeValue::Float(f) => Ok(*f),
        other => Err(format!("expected float, got {other:?}")),
    }
}

// ============================================================================
// Integer functions
// ============================================================================

fn builtin_math_abs(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    n.checked_abs()
        .map(NativeValue::Int)
        .ok_or_else(|| "MathError::Overflow: abs(i64::MIN) overflows".to_string())
}

fn builtin_math_min(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_int(&args[0])?;
    let b = expect_int(&args[1])?;
    Ok(NativeValue::Int(a.min(b)))
}

fn builtin_math_max(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_int(&args[0])?;
    let b = expect_int(&args[1])?;
    Ok(NativeValue::Int(a.max(b)))
}

fn builtin_math_clamp(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let n = expect_int(&args[0])?;
    let low = expect_int(&args[1])?;
    let high = expect_int(&args[2])?;
    Ok(NativeValue::Int(n.clamp(low, high)))
}

fn builtin_math_pow(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let base = expect_int(&args[0])?;
    let exp = expect_int(&args[1])?;
    if exp < 0 {
        return Err("MathError::NegativeExponent: pow requires non-negative exponent".to_string());
    }
    if exp > u32::MAX as i64 {
        return Err("MathError::Overflow: exponent exceeds u32::MAX".to_string());
    }
    base.checked_pow(exp as u32)
        .map(NativeValue::Int)
        .ok_or_else(|| "MathError::Overflow: pow result overflows i64".to_string())
}

fn builtin_math_gcd(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_int(&args[0])?;
    let b = expect_int(&args[1])?;
    Ok(NativeValue::Int(gcd_i64(a, b)))
}

fn builtin_math_lcm(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_int(&args[0])?;
    let b = expect_int(&args[1])?;
    if a == 0 || b == 0 {
        return Ok(NativeValue::Int(0));
    }
    let g = gcd_i64(a, b);
    let a_abs = a
        .checked_abs()
        .ok_or_else(|| "MathError::Overflow: lcm overflowed on abs".to_string())?;
    let b_abs = b
        .checked_abs()
        .ok_or_else(|| "MathError::Overflow: lcm overflowed on abs".to_string())?;
    (a_abs / g)
        .checked_mul(b_abs)
        .map(NativeValue::Int)
        .ok_or_else(|| "MathError::Overflow: lcm result overflows i64".to_string())
}

fn gcd_i64(a: i64, b: i64) -> i64 {
    let mut a = a.unsigned_abs();
    let mut b = b.unsigned_abs();
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a as i64
}

fn builtin_math_div_floor(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_int(&args[0])?;
    let b = expect_int(&args[1])?;
    if b == 0 {
        return Err("MathError::DivisionByZero: div_floor by zero".to_string());
    }
    // i64::div_euclid rounds toward negative infinity only when the remainder
    // is zero or non-negative, which is *not* the same as mathematical floor
    // division. Compute floor-division directly via truncated division plus
    // an adjustment when the remainder's sign disagrees with the divisor's.
    let q = a
        .checked_div(b)
        .ok_or_else(|| "MathError::Overflow: div_floor overflowed".to_string())?;
    let r = a % b;
    let adj = if (r != 0) && ((r < 0) != (b < 0)) { 1 } else { 0 };
    q.checked_sub(adj)
        .map(NativeValue::Int)
        .ok_or_else(|| "MathError::Overflow: div_floor overflowed".to_string())
}

fn builtin_math_div_ceil(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_int(&args[0])?;
    let b = expect_int(&args[1])?;
    if b == 0 {
        return Err("MathError::DivisionByZero: div_ceil by zero".to_string());
    }
    let q = a / b;
    let r = a % b;
    // If remainder is non-zero and signs of (r) and (b) match, add one.
    let adj = if (r != 0) && ((r < 0) == (b < 0)) { 1 } else { 0 };
    q.checked_add(adj)
        .map(NativeValue::Int)
        .ok_or_else(|| "MathError::Overflow: div_ceil overflowed".to_string())
}

fn builtin_math_rem_euclean(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_int(&args[0])?;
    let b = expect_int(&args[1])?;
    if b == 0 {
        return Err("MathError::DivisionByZero: rem_euclean by zero".to_string());
    }
    Ok(NativeValue::Int(a.rem_euclid(b)))
}

// ============================================================================
// Float functions
// ============================================================================

fn builtin_math_abs_f(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Float(expect_float(&args[0])?.abs()))
}

fn builtin_math_min_f(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_float(&args[0])?;
    let b = expect_float(&args[1])?;
    Ok(NativeValue::Float(a.min(b)))
}

fn builtin_math_max_f(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_float(&args[0])?;
    let b = expect_float(&args[1])?;
    Ok(NativeValue::Float(a.max(b)))
}

fn builtin_math_clamp_f(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let n = expect_float(&args[0])?;
    let low = expect_float(&args[1])?;
    let high = expect_float(&args[2])?;
    // f64::clamp panics on NaN bounds; guard explicitly.
    if low.is_nan() || high.is_nan() {
        return Err("MathError: clamp_f bounds must not be NaN".to_string());
    }
    if low > high {
        return Err("MathError: clamp_f low must be <= high".to_string());
    }
    Ok(NativeValue::Float(n.clamp(low, high)))
}

fn builtin_math_floor(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Float(expect_float(&args[0])?.floor()))
}

fn builtin_math_ceil(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Float(expect_float(&args[0])?.ceil()))
}

fn builtin_math_round(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Float(expect_float(&args[0])?.round()))
}

fn builtin_math_trunc(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Float(expect_float(&args[0])?.trunc()))
}

fn builtin_math_fract(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Float(expect_float(&args[0])?.fract()))
}

fn builtin_math_sqrt(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_float(&args[0])?;
    if n < 0.0 {
        return Err(format!("MathError::NegativeSqrt: sqrt({n}) is undefined"));
    }
    Ok(NativeValue::Float(n.sqrt()))
}

fn builtin_math_cbrt(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Float(expect_float(&args[0])?.cbrt()))
}

fn builtin_math_pow_f(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let base = expect_float(&args[0])?;
    let exp = expect_float(&args[1])?;
    Ok(NativeValue::Float(base.powf(exp)))
}

fn builtin_math_log(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let n = expect_float(&args[0])?;
    let base = expect_float(&args[1])?;
    if n <= 0.0 {
        return Err(format!("MathError::LogOfNonPositive: log({n}) is undefined"));
    }
    if base <= 0.0 || base == 1.0 {
        return Err(format!(
            "MathError: log base must be positive and != 1, got {base}"
        ));
    }
    Ok(NativeValue::Float(n.log(base)))
}

fn builtin_math_ln(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_float(&args[0])?;
    if n <= 0.0 {
        return Err(format!("MathError::LogOfNonPositive: ln({n}) is undefined"));
    }
    Ok(NativeValue::Float(n.ln()))
}

fn builtin_math_log2(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_float(&args[0])?;
    if n <= 0.0 {
        return Err(format!("MathError::LogOfNonPositive: log2({n}) is undefined"));
    }
    Ok(NativeValue::Float(n.log2()))
}

fn builtin_math_log10(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_float(&args[0])?;
    if n <= 0.0 {
        return Err(format!("MathError::LogOfNonPositive: log10({n}) is undefined"));
    }
    Ok(NativeValue::Float(n.log10()))
}

fn builtin_math_exp(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Float(expect_float(&args[0])?.exp()))
}

fn builtin_math_sin(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Float(expect_float(&args[0])?.sin()))
}

fn builtin_math_cos(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Float(expect_float(&args[0])?.cos()))
}

fn builtin_math_tan(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Float(expect_float(&args[0])?.tan()))
}

fn builtin_math_asin(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_float(&args[0])?;
    if n.abs() > 1.0 {
        return Err(format!("MathError: asin domain error, |{n}| > 1"));
    }
    Ok(NativeValue::Float(n.asin()))
}

fn builtin_math_acos(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_float(&args[0])?;
    if n.abs() > 1.0 {
        return Err(format!("MathError: acos domain error, |{n}| > 1"));
    }
    Ok(NativeValue::Float(n.acos()))
}

fn builtin_math_atan(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Float(expect_float(&args[0])?.atan()))
}

fn builtin_math_atan2(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let y = expect_float(&args[0])?;
    let x = expect_float(&args[1])?;
    Ok(NativeValue::Float(y.atan2(x)))
}

fn builtin_math_hypot(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_float(&args[0])?;
    let b = expect_float(&args[1])?;
    Ok(NativeValue::Float(a.hypot(b)))
}

fn builtin_math_is_nan(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Bool(expect_float(&args[0])?.is_nan()))
}

fn builtin_math_is_finite(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Bool(expect_float(&args[0])?.is_finite()))
}

fn builtin_math_is_inf(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    Ok(NativeValue::Bool(expect_float(&args[0])?.is_infinite()))
}

// ============================================================================
// Constants (zero-arg functions)
// ============================================================================

fn builtin_math_pi(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::Float(std::f64::consts::PI))
}

fn builtin_math_e(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::Float(std::f64::consts::E))
}

fn builtin_math_tau(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::Float(std::f64::consts::TAU))
}

fn builtin_math_inf(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::Float(f64::INFINITY))
}

fn builtin_math_nan(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::Float(f64::NAN))
}

// ============================================================================
// Conversion
// ============================================================================

fn builtin_math_int_to_float(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    Ok(NativeValue::Float(n as f64))
}

fn builtin_math_float_to_int(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_float(&args[0])?;
    // Spec: truncates toward zero. Guard NaN/Inf so we don't UB-cast.
    if n.is_nan() {
        return Err("MathError: float_to_int(NaN) is undefined".to_string());
    }
    if !n.is_finite() {
        return Err("MathError::Overflow: float_to_int of infinity".to_string());
    }
    let truncated = n.trunc();
    if truncated < (i64::MIN as f64) || truncated > (i64::MAX as f64) {
        return Err("MathError::Overflow: float out of i64 range".to_string());
    }
    Ok(NativeValue::Int(truncated as i64))
}

// ============================================================================
// Registration
// ============================================================================

/// Registers every `math` function with the registry under its `math_<name>`
/// native key.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    // Integer
    registry.register(interner.intern("math_abs"),         builtin_math_abs);
    registry.register(interner.intern("math_min"),         builtin_math_min);
    registry.register(interner.intern("math_max"),         builtin_math_max);
    registry.register(interner.intern("math_clamp"),       builtin_math_clamp);
    registry.register(interner.intern("math_pow"),         builtin_math_pow);
    registry.register(interner.intern("math_gcd"),         builtin_math_gcd);
    registry.register(interner.intern("math_lcm"),         builtin_math_lcm);
    registry.register(interner.intern("math_div_floor"),   builtin_math_div_floor);
    registry.register(interner.intern("math_div_ceil"),    builtin_math_div_ceil);
    registry.register(interner.intern("math_rem_euclean"), builtin_math_rem_euclean);

    // Float
    registry.register(interner.intern("math_abs_f"),       builtin_math_abs_f);
    registry.register(interner.intern("math_min_f"),       builtin_math_min_f);
    registry.register(interner.intern("math_max_f"),       builtin_math_max_f);
    registry.register(interner.intern("math_clamp_f"),     builtin_math_clamp_f);
    registry.register(interner.intern("math_floor"),       builtin_math_floor);
    registry.register(interner.intern("math_ceil"),        builtin_math_ceil);
    registry.register(interner.intern("math_round"),       builtin_math_round);
    registry.register(interner.intern("math_trunc"),       builtin_math_trunc);
    registry.register(interner.intern("math_fract"),       builtin_math_fract);
    registry.register(interner.intern("math_sqrt"),        builtin_math_sqrt);
    registry.register(interner.intern("math_cbrt"),        builtin_math_cbrt);
    registry.register(interner.intern("math_pow_f"),       builtin_math_pow_f);
    registry.register(interner.intern("math_log"),         builtin_math_log);
    registry.register(interner.intern("math_ln"),          builtin_math_ln);
    registry.register(interner.intern("math_log2"),        builtin_math_log2);
    registry.register(interner.intern("math_log10"),       builtin_math_log10);
    registry.register(interner.intern("math_exp"),         builtin_math_exp);
    registry.register(interner.intern("math_sin"),         builtin_math_sin);
    registry.register(interner.intern("math_cos"),         builtin_math_cos);
    registry.register(interner.intern("math_tan"),         builtin_math_tan);
    registry.register(interner.intern("math_asin"),        builtin_math_asin);
    registry.register(interner.intern("math_acos"),        builtin_math_acos);
    registry.register(interner.intern("math_atan"),        builtin_math_atan);
    registry.register(interner.intern("math_atan2"),       builtin_math_atan2);
    registry.register(interner.intern("math_hypot"),       builtin_math_hypot);
    registry.register(interner.intern("math_is_nan"),      builtin_math_is_nan);
    registry.register(interner.intern("math_is_finite"),   builtin_math_is_finite);
    registry.register(interner.intern("math_is_inf"),      builtin_math_is_inf);

    // Constants
    registry.register(interner.intern("math_PI"),          builtin_math_pi);
    registry.register(interner.intern("math_E"),           builtin_math_e);
    registry.register(interner.intern("math_TAU"),         builtin_math_tau);
    registry.register(interner.intern("math_INF"),         builtin_math_inf);
    registry.register(interner.intern("math_NAN"),         builtin_math_nan);

    // Conversion
    registry.register(interner.intern("math_int_to_float"), builtin_math_int_to_float);
    registry.register(interner.intern("math_float_to_int"), builtin_math_float_to_int);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-10;

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < EPS
    }

    fn int(n: i64) -> NativeValue {
        NativeValue::Int(n)
    }

    fn float(n: f64) -> NativeValue {
        NativeValue::Float(n)
    }

    fn unwrap_int(v: NativeValue) -> i64 {
        match v {
            NativeValue::Int(n) => n,
            other => panic!("expected Int, got {other:?}"),
        }
    }

    fn unwrap_float(v: NativeValue) -> f64 {
        match v {
            NativeValue::Float(n) => n,
            other => panic!("expected Float, got {other:?}"),
        }
    }

    fn unwrap_bool(v: NativeValue) -> bool {
        match v {
            NativeValue::Bool(b) => b,
            other => panic!("expected Bool, got {other:?}"),
        }
    }

    // ----- Integer ----------------------------------------------------------

    #[test]
    fn abs_basics() {
        assert_eq!(unwrap_int(builtin_math_abs(&[int(-5)]).unwrap()), 5);
        assert_eq!(unwrap_int(builtin_math_abs(&[int(0)]).unwrap()), 0);
        assert_eq!(unwrap_int(builtin_math_abs(&[int(7)]).unwrap()), 7);
    }

    #[test]
    fn abs_overflow_errors() {
        assert!(builtin_math_abs(&[int(i64::MIN)]).is_err());
    }

    #[test]
    fn min_max_basics() {
        assert_eq!(unwrap_int(builtin_math_min(&[int(3), int(7)]).unwrap()), 3);
        assert_eq!(unwrap_int(builtin_math_max(&[int(3), int(7)]).unwrap()), 7);
    }

    #[test]
    fn clamp_basics() {
        assert_eq!(
            unwrap_int(builtin_math_clamp(&[int(5), int(0), int(10)]).unwrap()),
            5
        );
        assert_eq!(
            unwrap_int(builtin_math_clamp(&[int(-1), int(0), int(10)]).unwrap()),
            0
        );
        assert_eq!(
            unwrap_int(builtin_math_clamp(&[int(20), int(0), int(10)]).unwrap()),
            10
        );
    }

    #[test]
    fn pow_basics() {
        assert_eq!(unwrap_int(builtin_math_pow(&[int(2), int(10)]).unwrap()), 1024);
        assert_eq!(unwrap_int(builtin_math_pow(&[int(3), int(0)]).unwrap()), 1);
    }

    #[test]
    fn pow_negative_exp_errors() {
        let err = builtin_math_pow(&[int(2), int(-1)]).unwrap_err();
        assert!(err.contains("NegativeExponent"));
    }

    #[test]
    fn pow_overflow_errors() {
        assert!(builtin_math_pow(&[int(2), int(63)]).is_err());
    }

    #[test]
    fn gcd_basics() {
        assert_eq!(unwrap_int(builtin_math_gcd(&[int(12), int(8)]).unwrap()), 4);
        assert_eq!(unwrap_int(builtin_math_gcd(&[int(0), int(5)]).unwrap()), 5);
        assert_eq!(unwrap_int(builtin_math_gcd(&[int(0), int(0)]).unwrap()), 0);
        assert_eq!(unwrap_int(builtin_math_gcd(&[int(-12), int(8)]).unwrap()), 4);
    }

    #[test]
    fn lcm_basics() {
        assert_eq!(unwrap_int(builtin_math_lcm(&[int(4), int(6)]).unwrap()), 12);
        assert_eq!(unwrap_int(builtin_math_lcm(&[int(0), int(5)]).unwrap()), 0);
        assert_eq!(unwrap_int(builtin_math_lcm(&[int(5), int(0)]).unwrap()), 0);
    }

    #[test]
    fn div_floor_basics() {
        assert_eq!(
            unwrap_int(builtin_math_div_floor(&[int(7), int(2)]).unwrap()),
            3
        );
        assert_eq!(
            unwrap_int(builtin_math_div_floor(&[int(-7), int(2)]).unwrap()),
            -4
        );
        assert_eq!(
            unwrap_int(builtin_math_div_floor(&[int(7), int(-2)]).unwrap()),
            -4
        );
        assert_eq!(
            unwrap_int(builtin_math_div_floor(&[int(-7), int(-2)]).unwrap()),
            3
        );
        assert_eq!(
            unwrap_int(builtin_math_div_floor(&[int(8), int(2)]).unwrap()),
            4
        );
    }

    #[test]
    fn div_floor_zero_errors() {
        let err = builtin_math_div_floor(&[int(7), int(0)]).unwrap_err();
        assert!(err.contains("DivisionByZero"));
    }

    #[test]
    fn div_ceil_basics() {
        assert_eq!(
            unwrap_int(builtin_math_div_ceil(&[int(7), int(2)]).unwrap()),
            4
        );
        assert_eq!(
            unwrap_int(builtin_math_div_ceil(&[int(-7), int(2)]).unwrap()),
            -3
        );
        assert_eq!(
            unwrap_int(builtin_math_div_ceil(&[int(8), int(2)]).unwrap()),
            4
        );
        assert_eq!(
            unwrap_int(builtin_math_div_ceil(&[int(-8), int(-2)]).unwrap()),
            4
        );
    }

    #[test]
    fn div_ceil_zero_errors() {
        let err = builtin_math_div_ceil(&[int(7), int(0)]).unwrap_err();
        assert!(err.contains("DivisionByZero"));
    }

    #[test]
    fn rem_euclean_basics() {
        assert_eq!(
            unwrap_int(builtin_math_rem_euclean(&[int(-7), int(3)]).unwrap()),
            2
        );
        assert_eq!(
            unwrap_int(builtin_math_rem_euclean(&[int(7), int(3)]).unwrap()),
            1
        );
        assert_eq!(
            unwrap_int(builtin_math_rem_euclean(&[int(0), int(3)]).unwrap()),
            0
        );
    }

    #[test]
    fn rem_euclean_zero_errors() {
        assert!(builtin_math_rem_euclean(&[int(7), int(0)]).is_err());
    }

    // ----- Float rounding ---------------------------------------------------

    #[test]
    fn floor_basic() {
        assert!(approx_eq(
            unwrap_float(builtin_math_floor(&[float(1.7)]).unwrap()),
            1.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_floor(&[float(-1.2)]).unwrap()),
            -2.0
        ));
    }

    #[test]
    fn ceil_basic() {
        assert!(approx_eq(
            unwrap_float(builtin_math_ceil(&[float(1.2)]).unwrap()),
            2.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_ceil(&[float(-1.7)]).unwrap()),
            -1.0
        ));
    }

    #[test]
    fn round_basic() {
        assert!(approx_eq(
            unwrap_float(builtin_math_round(&[float(1.5)]).unwrap()),
            2.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_round(&[float(1.4)]).unwrap()),
            1.0
        ));
    }

    #[test]
    fn trunc_and_fract() {
        assert!(approx_eq(
            unwrap_float(builtin_math_trunc(&[float(3.9)]).unwrap()),
            3.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_trunc(&[float(-3.9)]).unwrap()),
            -3.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_fract(&[float(3.25)]).unwrap()),
            0.25
        ));
    }

    // ----- Float math -------------------------------------------------------

    #[test]
    fn abs_min_max_clamp_f() {
        assert!(approx_eq(
            unwrap_float(builtin_math_abs_f(&[float(-2.5)]).unwrap()),
            2.5
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_min_f(&[float(1.0), float(2.0)]).unwrap()),
            1.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_max_f(&[float(1.0), float(2.0)]).unwrap()),
            2.0
        ));
        assert!(approx_eq(
            unwrap_float(
                builtin_math_clamp_f(&[float(5.0), float(0.0), float(3.0)]).unwrap()
            ),
            3.0
        ));
    }

    #[test]
    fn clamp_f_invalid_bounds_errors() {
        assert!(
            builtin_math_clamp_f(&[float(1.0), float(5.0), float(0.0)])
                .is_err()
        );
        assert!(
            builtin_math_clamp_f(&[float(1.0), float(f64::NAN), float(0.0)])
                .is_err()
        );
    }

    #[test]
    fn sqrt_basic() {
        assert!(approx_eq(
            unwrap_float(builtin_math_sqrt(&[float(4.0)]).unwrap()),
            2.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_sqrt(&[float(0.0)]).unwrap()),
            0.0
        ));
    }

    #[test]
    fn sqrt_negative_errors() {
        let err = builtin_math_sqrt(&[float(-1.0)]).unwrap_err();
        assert!(err.contains("NegativeSqrt"));
    }

    #[test]
    fn cbrt_basic() {
        assert!(approx_eq(
            unwrap_float(builtin_math_cbrt(&[float(27.0)]).unwrap()),
            3.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_cbrt(&[float(-8.0)]).unwrap()),
            -2.0
        ));
    }

    #[test]
    fn pow_f_basic() {
        assert!(approx_eq(
            unwrap_float(builtin_math_pow_f(&[float(2.0), float(10.0)]).unwrap()),
            1024.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_pow_f(&[float(2.0), float(-1.0)]).unwrap()),
            0.5
        ));
    }

    #[test]
    fn log_basic_and_errors() {
        assert!(approx_eq(
            unwrap_float(builtin_math_log(&[float(100.0), float(10.0)]).unwrap()),
            2.0
        ));
        assert!(builtin_math_log(&[float(-1.0), float(10.0)]).is_err());
        assert!(builtin_math_log(&[float(10.0), float(0.0)]).is_err());
        assert!(builtin_math_log(&[float(10.0), float(1.0)]).is_err());
        assert!(builtin_math_log(&[float(10.0), float(-1.0)]).is_err());
    }

    #[test]
    fn ln_basic_and_errors() {
        assert!(approx_eq(
            unwrap_float(builtin_math_ln(&[float(std::f64::consts::E)]).unwrap()),
            1.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_ln(&[float(1.0)]).unwrap()),
            0.0
        ));
        assert!(builtin_math_ln(&[float(0.0)]).is_err());
        assert!(builtin_math_ln(&[float(-1.0)]).is_err());
    }

    #[test]
    fn log2_basic_and_errors() {
        assert!(approx_eq(
            unwrap_float(builtin_math_log2(&[float(8.0)]).unwrap()),
            3.0
        ));
        assert!(builtin_math_log2(&[float(0.0)]).is_err());
    }

    #[test]
    fn log10_basic_and_errors() {
        assert!(approx_eq(
            unwrap_float(builtin_math_log10(&[float(1000.0)]).unwrap()),
            3.0
        ));
        assert!(builtin_math_log10(&[float(-1.0)]).is_err());
    }

    #[test]
    fn exp_basic() {
        assert!(approx_eq(
            unwrap_float(builtin_math_exp(&[float(0.0)]).unwrap()),
            1.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_exp(&[float(1.0)]).unwrap()),
            std::f64::consts::E
        ));
    }

    // ----- Trig -------------------------------------------------------------

    #[test]
    fn sin_cos_tan_basic() {
        assert!(approx_eq(
            unwrap_float(builtin_math_sin(&[float(0.0)]).unwrap()),
            0.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_cos(&[float(0.0)]).unwrap()),
            1.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_tan(&[float(0.0)]).unwrap()),
            0.0
        ));
    }

    #[test]
    fn asin_acos_basic_and_errors() {
        assert!(approx_eq(
            unwrap_float(builtin_math_asin(&[float(0.0)]).unwrap()),
            0.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_acos(&[float(1.0)]).unwrap()),
            0.0
        ));
        assert!(builtin_math_asin(&[float(2.0)]).is_err());
        assert!(builtin_math_acos(&[float(-2.0)]).is_err());
    }

    #[test]
    fn atan_atan2_basic() {
        assert!(approx_eq(
            unwrap_float(builtin_math_atan(&[float(0.0)]).unwrap()),
            0.0
        ));
        let v = unwrap_float(builtin_math_atan2(&[float(1.0), float(1.0)]).unwrap());
        assert!(approx_eq(v, std::f64::consts::FRAC_PI_4));
    }

    #[test]
    fn hypot_basic() {
        assert!(approx_eq(
            unwrap_float(builtin_math_hypot(&[float(3.0), float(4.0)]).unwrap()),
            5.0
        ));
    }

    // ----- Predicates -------------------------------------------------------

    #[test]
    fn is_nan_finite_inf() {
        assert!(unwrap_bool(builtin_math_is_nan(&[float(f64::NAN)]).unwrap()));
        assert!(!unwrap_bool(builtin_math_is_nan(&[float(1.0)]).unwrap()));
        assert!(!unwrap_bool(
            builtin_math_is_finite(&[float(f64::INFINITY)]).unwrap()
        ));
        assert!(unwrap_bool(builtin_math_is_finite(&[float(1.0)]).unwrap()));
        assert!(unwrap_bool(
            builtin_math_is_inf(&[float(f64::INFINITY)]).unwrap()
        ));
        assert!(unwrap_bool(
            builtin_math_is_inf(&[float(f64::NEG_INFINITY)]).unwrap()
        ));
        assert!(!unwrap_bool(builtin_math_is_inf(&[float(1.0)]).unwrap()));
    }

    // ----- Constants --------------------------------------------------------

    #[test]
    fn constants_have_correct_values() {
        assert!(approx_eq(
            unwrap_float(builtin_math_pi(&[]).unwrap()),
            std::f64::consts::PI
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_e(&[]).unwrap()),
            std::f64::consts::E
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_tau(&[]).unwrap()),
            std::f64::consts::TAU
        ));
        assert!(unwrap_float(builtin_math_inf(&[]).unwrap()).is_infinite());
        assert!(unwrap_float(builtin_math_nan(&[]).unwrap()).is_nan());
    }

    #[test]
    fn constants_reject_args() {
        assert!(builtin_math_pi(&[int(0)]).is_err());
    }

    // ----- Conversion -------------------------------------------------------

    #[test]
    fn int_to_float_basic() {
        assert!(approx_eq(
            unwrap_float(builtin_math_int_to_float(&[int(5)]).unwrap()),
            5.0
        ));
        assert!(approx_eq(
            unwrap_float(builtin_math_int_to_float(&[int(-3)]).unwrap()),
            -3.0
        ));
    }

    #[test]
    fn float_to_int_truncates_toward_zero() {
        assert_eq!(
            unwrap_int(builtin_math_float_to_int(&[float(3.9)]).unwrap()),
            3
        );
        assert_eq!(
            unwrap_int(builtin_math_float_to_int(&[float(-3.9)]).unwrap()),
            -3
        );
        assert_eq!(
            unwrap_int(builtin_math_float_to_int(&[float(0.0)]).unwrap()),
            0
        );
    }

    #[test]
    fn float_to_int_errors_on_nonfinite() {
        assert!(builtin_math_float_to_int(&[float(f64::NAN)]).is_err());
        assert!(builtin_math_float_to_int(&[float(f64::INFINITY)]).is_err());
        assert!(builtin_math_float_to_int(&[float(1e30)]).is_err());
    }

    // ----- Arg-count guards -------------------------------------------------

    #[test]
    fn arg_count_mismatch_errors() {
        assert!(builtin_math_abs(&[]).is_err());
        assert!(builtin_math_min(&[int(1)]).is_err());
        assert!(builtin_math_clamp(&[int(1), int(2)]).is_err());
        assert!(builtin_math_sqrt(&[float(1.0), float(2.0)]).is_err());
    }

    // ----- Registration round-trip -----------------------------------------

    #[test]
    fn register_populates_every_function() {
        let mut registry = NativeRegistry::new();
        let mut interner = Interner::new();
        register(&mut registry, &mut interner);

        let names = [
            // Integer
            "math_abs", "math_min", "math_max", "math_clamp", "math_pow",
            "math_gcd", "math_lcm", "math_div_floor", "math_div_ceil",
            "math_rem_euclean",
            // Float
            "math_abs_f", "math_min_f", "math_max_f", "math_clamp_f",
            "math_floor", "math_ceil", "math_round", "math_trunc",
            "math_fract", "math_sqrt", "math_cbrt", "math_pow_f",
            "math_log", "math_ln", "math_log2", "math_log10", "math_exp",
            "math_sin", "math_cos", "math_tan", "math_asin", "math_acos",
            "math_atan", "math_atan2", "math_hypot", "math_is_nan",
            "math_is_finite", "math_is_inf",
            // Constants
            "math_PI", "math_E", "math_TAU", "math_INF", "math_NAN",
            // Conversion
            "math_int_to_float", "math_float_to_int",
        ];

        for name in names {
            let sym = interner.intern(name);
            assert!(
                registry.get(sym).is_some(),
                "missing registration for {name}"
            );
        }
    }
}
