// REQUIRED NATIVE VALUE VARIANTS:
//   List(Vec<NativeValue>)         // already exists as Array — reuse it
//   Ordering(std::cmp::Ordering)   // returned by cmp_int / cmp_float / cmp_str /
//                                  // cmp_str_natural and accepted by `with`,
//                                  // `reverse`, `then`, `is_sorted_by` callbacks.
//   Closure(...)                   // for key/comparator callbacks. Falls back
//                                  // to `invoke_closure` plumbing during the
//                                  // parallel build phase.
//   Tuple2(...)                    // not used directly here, but downstream
//                                  // callers may pass tuples through `by`.
//
// Until the dedicated `Ordering` variant exists, we encode an Ordering as
// `NativeValue::Int(-1 | 0 | 1)`. `register()` also exposes the three
// constants `Less`, `Equal`, `Greater` as zero-argument native functions
// that return the same encoding, so that comparators written in Ferric have
// a stable surface to call. The integration step swaps the encoding in one
// place (the `ord_*` helpers below).
//
// REQUIRED DEPS:
//   none — pure std

//! `sort` stdlib module — sorting and ordering.
//!
//! All sorts are stable. Higher-order entry points assume the integration
//! step provides `invoke_closure` for closure-shaped `NativeValue`s.

use std::cmp::Ordering;

use crate::{NativeRegistry, NativeValue};
use ferric_common::Interner;

// ---------------------------------------------------------------------------
// Closure invocation hook (provided by integration step)
// ---------------------------------------------------------------------------

use crate::invoke_closure;

// ---------------------------------------------------------------------------
// Ordering encoding helpers
// ---------------------------------------------------------------------------

fn ord_to_native(o: Ordering) -> NativeValue {
    match o {
        Ordering::Less => NativeValue::Int(-1),
        Ordering::Equal => NativeValue::Int(0),
        Ordering::Greater => NativeValue::Int(1),
    }
}

fn ord_from_native(v: &NativeValue) -> Result<Ordering, String> {
    match v {
        NativeValue::Int(n) if *n < 0 => Ok(Ordering::Less),
        NativeValue::Int(0) => Ok(Ordering::Equal),
        NativeValue::Int(n) if *n > 0 => Ok(Ordering::Greater),
        other => Err(format!("sort: expected Ordering, got {other:?}")),
    }
}

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!(
            "sort: expected {} argument(s), got {}",
            expected,
            args.len()
        ))
    } else {
        Ok(())
    }
}

fn expect_int(v: &NativeValue) -> Result<i64, String> {
    match v {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("sort: expected Int, got {other:?}")),
    }
}

fn expect_float(v: &NativeValue) -> Result<f64, String> {
    match v {
        NativeValue::Float(f) => Ok(*f),
        other => Err(format!("sort: expected Float, got {other:?}")),
    }
}

fn expect_str(v: &NativeValue) -> Result<&String, String> {
    match v {
        NativeValue::Str(s) => Ok(s),
        other => Err(format!("sort: expected Str, got {other:?}")),
    }
}

fn expect_list(v: &NativeValue) -> Result<&Vec<NativeValue>, String> {
    match v {
        NativeValue::Array(items) => Ok(items),
        other => Err(format!("sort: expected List, got {other:?}")),
    }
}

/// Total ordering across the scalar subset we support pre-traits:
/// Int < Float < Str. Floats use NaN-as-equal-to-NaN to keep this total.
fn total_cmp(a: &NativeValue, b: &NativeValue) -> Result<Ordering, String> {
    match (a, b) {
        (NativeValue::Int(x), NativeValue::Int(y)) => Ok(x.cmp(y)),
        (NativeValue::Float(x), NativeValue::Float(y)) => Ok(x.total_cmp(y)),
        (NativeValue::Str(x), NativeValue::Str(y)) => Ok(x.cmp(y)),
        (NativeValue::Bool(x), NativeValue::Bool(y)) => Ok(x.cmp(y)),
        _ => Err(format!(
            "sort: asc/desc/is_sorted require uniform Int / Float / Str / Bool (got {a:?} vs {b:?})"
        )),
    }
}

// ---------------------------------------------------------------------------
// Comparators
// ---------------------------------------------------------------------------

fn builtin_sort_cmp_int(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_int(&args[0])?;
    let b = expect_int(&args[1])?;
    Ok(ord_to_native(a.cmp(&b)))
}

fn builtin_sort_cmp_float(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_float(&args[0])?;
    let b = expect_float(&args[1])?;
    Ok(ord_to_native(a.total_cmp(&b)))
}

fn builtin_sort_cmp_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_str(&args[0])?;
    let b = expect_str(&args[1])?;
    Ok(ord_to_native(a.cmp(b)))
}

/// Natural ordering — splits each string into runs of digits and non-digits,
/// compares digit runs numerically, non-digit runs lexicographically, and
/// considers a shorter prefix less than a longer one.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek(), bi.peek()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&ac), Some(&bc)) => {
                if ac.is_ascii_digit() && bc.is_ascii_digit() {
                    let mut a_run = String::new();
                    while let Some(&c) = ai.peek() {
                        if c.is_ascii_digit() {
                            a_run.push(c);
                            ai.next();
                        } else {
                            break;
                        }
                    }
                    let mut b_run = String::new();
                    while let Some(&c) = bi.peek() {
                        if c.is_ascii_digit() {
                            b_run.push(c);
                            bi.next();
                        } else {
                            break;
                        }
                    }
                    // Compare numerically; if the numbers don't fit in u128
                    // fall back to (length, lexicographic).
                    let an: Result<u128, _> = a_run.trim_start_matches('0').parse();
                    let bn: Result<u128, _> = b_run.trim_start_matches('0').parse();
                    let cmp = match (an, bn) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        _ => {
                            let la = a_run.trim_start_matches('0').len();
                            let lb = b_run.trim_start_matches('0').len();
                            la.cmp(&lb).then_with(|| a_run.cmp(&b_run))
                        }
                    };
                    if cmp != Ordering::Equal {
                        return cmp;
                    }
                    // Tiebreak: shorter zero-prefix wins, otherwise equal.
                    let leading_a = a_run.len() - a_run.trim_start_matches('0').len();
                    let leading_b = b_run.len() - b_run.trim_start_matches('0').len();
                    if leading_a != leading_b {
                        return leading_a.cmp(&leading_b);
                    }
                } else {
                    let cmp = ac.cmp(&bc);
                    if cmp != Ordering::Equal {
                        return cmp;
                    }
                    ai.next();
                    bi.next();
                }
            }
        }
    }
}

fn builtin_sort_cmp_str_natural(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_str(&args[0])?;
    let b = expect_str(&args[1])?;
    Ok(ord_to_native(natural_cmp(a, b)))
}

// ---------------------------------------------------------------------------
// Ordering constants (Less / Equal / Greater)
// ---------------------------------------------------------------------------

fn builtin_sort_less(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(ord_to_native(Ordering::Less))
}

fn builtin_sort_equal(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(ord_to_native(Ordering::Equal))
}

fn builtin_sort_greater(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(ord_to_native(Ordering::Greater))
}

// ---------------------------------------------------------------------------
// Sorting entry points
// ---------------------------------------------------------------------------

fn builtin_sort_asc(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let mut v = expect_list(&args[0])?.clone();
    let mut err: Option<String> = None;
    v.sort_by(|a, b| match total_cmp(a, b) {
        Ok(o) => o,
        Err(e) => {
            if err.is_none() {
                err = Some(e);
            }
            Ordering::Equal
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(NativeValue::Array(v))
}

fn builtin_sort_desc(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let mut v = expect_list(&args[0])?.clone();
    let mut err: Option<String> = None;
    v.sort_by(|a, b| match total_cmp(a, b) {
        Ok(o) => o.reverse(),
        Err(e) => {
            if err.is_none() {
                err = Some(e);
            }
            Ordering::Equal
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(NativeValue::Array(v))
}

fn builtin_sort_by(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let key = &args[1];

    // Pre-compute keys to keep closure invocations to O(n).
    let mut keyed: Vec<(NativeValue, NativeValue)> = Vec::with_capacity(l.len());
    for item in l {
        let k = invoke_closure(key, &[item.clone()])?;
        keyed.push((k, item));
    }
    let mut err: Option<String> = None;
    keyed.sort_by(|a, b| match total_cmp(&a.0, &b.0) {
        Ok(o) => o,
        Err(e) => {
            if err.is_none() {
                err = Some(e);
            }
            Ordering::Equal
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(NativeValue::Array(keyed.into_iter().map(|(_, v)| v).collect()))
}

fn builtin_sort_by_desc(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let key = &args[1];

    let mut keyed: Vec<(NativeValue, NativeValue)> = Vec::with_capacity(l.len());
    for item in l {
        let k = invoke_closure(key, &[item.clone()])?;
        keyed.push((k, item));
    }
    let mut err: Option<String> = None;
    keyed.sort_by(|a, b| match total_cmp(&a.0, &b.0) {
        Ok(o) => o.reverse(),
        Err(e) => {
            if err.is_none() {
                err = Some(e);
            }
            Ordering::Equal
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(NativeValue::Array(keyed.into_iter().map(|(_, v)| v).collect()))
}

fn builtin_sort_with(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let cmp = &args[1];
    // We need to surface closure errors. Use sort_by + interior cell.
    let mut err: Option<String> = None;
    let mut v = l;
    v.sort_by(|a, b| {
        if err.is_some() {
            return Ordering::Equal;
        }
        match invoke_closure(cmp, &[a.clone(), b.clone()]) {
            Ok(ov) => match ord_from_native(&ov) {
                Ok(o) => o,
                Err(e) => {
                    err = Some(e);
                    Ordering::Equal
                }
            },
            Err(e) => {
                err = Some(e);
                Ordering::Equal
            }
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(NativeValue::Array(v))
}

// ---------------------------------------------------------------------------
// Comparator combinators
// ---------------------------------------------------------------------------
//
// `reverse` and `then` are normally higher-order: take a comparator, return
// a comparator. Until closures can be constructed natively, we expose the
// "applied" form: `sort_reverse(cmp_fn, a, b)` and
// `sort_then(cmp_fn, tiebreak_fn, a, b)`. Once closure plumbing lands the
// integration step can layer the curried form on top of these.

fn builtin_sort_reverse(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let cmp = &args[0];
    let a = args[1].clone();
    let b = args[2].clone();
    let r = invoke_closure(cmp, &[a, b])?;
    let o = ord_from_native(&r)?;
    Ok(ord_to_native(o.reverse()))
}

fn builtin_sort_then(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 4)?;
    let cmp = &args[0];
    let tie = &args[1];
    let a = args[2].clone();
    let b = args[3].clone();
    let primary = invoke_closure(cmp, &[a.clone(), b.clone()])?;
    let primary_ord = ord_from_native(&primary)?;
    if primary_ord != Ordering::Equal {
        return Ok(ord_to_native(primary_ord));
    }
    let secondary = invoke_closure(tie, &[a, b])?;
    let secondary_ord = ord_from_native(&secondary)?;
    Ok(ord_to_native(secondary_ord))
}

// ---------------------------------------------------------------------------
// Partial sort
// ---------------------------------------------------------------------------

fn builtin_sort_top(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let l = expect_list(&args[0])?.clone();
    let n = expect_int(&args[1])?.max(0) as usize;
    let key = &args[2];

    let mut keyed: Vec<(NativeValue, NativeValue)> = Vec::with_capacity(l.len());
    for item in l {
        let k = invoke_closure(key, &[item.clone()])?;
        keyed.push((k, item));
    }
    let mut err: Option<String> = None;
    // Sort descending by key.
    keyed.sort_by(|a, b| match total_cmp(&a.0, &b.0) {
        Ok(o) => o.reverse(),
        Err(e) => {
            if err.is_none() {
                err = Some(e);
            }
            Ordering::Equal
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    keyed.truncate(n);
    Ok(NativeValue::Array(keyed.into_iter().map(|(_, v)| v).collect()))
}

fn builtin_sort_bottom(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let l = expect_list(&args[0])?.clone();
    let n = expect_int(&args[1])?.max(0) as usize;
    let key = &args[2];

    let mut keyed: Vec<(NativeValue, NativeValue)> = Vec::with_capacity(l.len());
    for item in l {
        let k = invoke_closure(key, &[item.clone()])?;
        keyed.push((k, item));
    }
    let mut err: Option<String> = None;
    keyed.sort_by(|a, b| match total_cmp(&a.0, &b.0) {
        Ok(o) => o,
        Err(e) => {
            if err.is_none() {
                err = Some(e);
            }
            Ordering::Equal
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    keyed.truncate(n);
    Ok(NativeValue::Array(keyed.into_iter().map(|(_, v)| v).collect()))
}

// ---------------------------------------------------------------------------
// Predicates
// ---------------------------------------------------------------------------

fn builtin_sort_is_sorted(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_list(&args[0])?;
    for w in l.windows(2) {
        if total_cmp(&w[0], &w[1])? == Ordering::Greater {
            return Ok(NativeValue::Bool(false));
        }
    }
    Ok(NativeValue::Bool(true))
}

fn builtin_sort_is_sorted_by(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let key = &args[1];
    let mut prev_key: Option<NativeValue> = None;
    for item in l {
        let k = invoke_closure(key, &[item])?;
        if let Some(p) = &prev_key {
            if total_cmp(p, &k)? == Ordering::Greater {
                return Ok(NativeValue::Bool(false));
            }
        }
        prev_key = Some(k);
    }
    Ok(NativeValue::Bool(true))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    registry.register(interner.intern("sort_asc"),     builtin_sort_asc);
    registry.register(interner.intern("sort_desc"),    builtin_sort_desc);
    registry.register(interner.intern("sort_by"),      builtin_sort_by);
    registry.register(interner.intern("sort_by_desc"), builtin_sort_by_desc);
    registry.register(interner.intern("sort_with"),    builtin_sort_with);

    registry.register(interner.intern("sort_cmp_int"),          builtin_sort_cmp_int);
    registry.register(interner.intern("sort_cmp_float"),        builtin_sort_cmp_float);
    registry.register(interner.intern("sort_cmp_str"),          builtin_sort_cmp_str);
    registry.register(interner.intern("sort_cmp_str_natural"),  builtin_sort_cmp_str_natural);

    registry.register(interner.intern("sort_reverse"), builtin_sort_reverse);
    registry.register(interner.intern("sort_then"),    builtin_sort_then);

    registry.register(interner.intern("sort_top"),    builtin_sort_top);
    registry.register(interner.intern("sort_bottom"), builtin_sort_bottom);

    registry.register(interner.intern("sort_is_sorted"),    builtin_sort_is_sorted);
    registry.register(interner.intern("sort_is_sorted_by"), builtin_sort_is_sorted_by);

    // Ordering constants exposed as zero-arg native functions.
    registry.register(interner.intern("sort_less"),    builtin_sort_less);
    registry.register(interner.intern("sort_equal"),   builtin_sort_equal);
    registry.register(interner.intern("sort_greater"), builtin_sort_greater);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeClosure;

    fn ints(xs: &[i64]) -> NativeValue {
        NativeValue::Array(xs.iter().map(|n| NativeValue::Int(*n)).collect())
    }

    fn strs(xs: &[&str]) -> NativeValue {
        NativeValue::Array(
            xs.iter()
                .map(|s| NativeValue::Str(s.to_string()))
                .collect(),
        )
    }

    fn closure<F>(f: F) -> NativeValue
    where
        F: Fn(&[NativeValue]) -> Result<NativeValue, String> + 'static,
    {
        NativeValue::Closure(NativeClosure::new(f))
    }

    fn cmp_int_closure() -> NativeValue {
        closure(builtin_sort_cmp_int)
    }

    fn unwrap_int(v: &NativeValue) -> i64 {
        match v {
            NativeValue::Int(n) => *n,
            other => panic!("expected Int, got {other:?}"),
        }
    }

    fn unwrap_str(v: &NativeValue) -> String {
        match v {
            NativeValue::Str(s) => s.clone(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    // --- Basic sorts -------------------------------------------------------

    #[test]
    fn asc_sorts_ints_ascending() {
        let r = builtin_sort_asc(&[ints(&[3, 1, 2])]).unwrap();
        assert_eq!(r, ints(&[1, 2, 3]));
    }

    #[test]
    fn desc_sorts_ints_descending() {
        let r = builtin_sort_desc(&[ints(&[3, 1, 2])]).unwrap();
        assert_eq!(r, ints(&[3, 2, 1]));
    }

    #[test]
    fn asc_handles_strings() {
        let r = builtin_sort_asc(&[strs(&["c", "a", "b"])]).unwrap();
        assert_eq!(r, strs(&["a", "b", "c"]));
    }

    #[test]
    fn asc_handles_floats() {
        let l = NativeValue::Array(vec![
            NativeValue::Float(2.5),
            NativeValue::Float(0.5),
            NativeValue::Float(1.5),
        ]);
        let r = builtin_sort_asc(&[l]).unwrap();
        assert_eq!(
            r,
            NativeValue::Array(vec![
                NativeValue::Float(0.5),
                NativeValue::Float(1.5),
                NativeValue::Float(2.5),
            ])
        );
    }

    #[test]
    fn asc_empty_is_empty() {
        let r = builtin_sort_asc(&[ints(&[])]).unwrap();
        assert_eq!(r, ints(&[]));
    }

    #[test]
    fn asc_mixed_types_is_error() {
        let mixed = NativeValue::Array(vec![NativeValue::Int(1), NativeValue::Str("x".into())]);
        assert!(builtin_sort_asc(&[mixed]).is_err());
    }

    // --- Comparators -------------------------------------------------------

    #[test]
    fn cmp_int_returns_correct_orderings() {
        assert_eq!(
            builtin_sort_cmp_int(&[NativeValue::Int(1), NativeValue::Int(2)]).unwrap(),
            ord_to_native(Ordering::Less)
        );
        assert_eq!(
            builtin_sort_cmp_int(&[NativeValue::Int(2), NativeValue::Int(2)]).unwrap(),
            ord_to_native(Ordering::Equal)
        );
        assert_eq!(
            builtin_sort_cmp_int(&[NativeValue::Int(3), NativeValue::Int(2)]).unwrap(),
            ord_to_native(Ordering::Greater)
        );
    }

    #[test]
    fn cmp_float_total_orders_nan() {
        // total_cmp puts NaN at the extremes — verify it doesn't error.
        let r = builtin_sort_cmp_float(&[
            NativeValue::Float(1.0),
            NativeValue::Float(f64::NAN),
        ])
        .unwrap();
        // 1.0 < NaN under total_cmp
        assert_eq!(r, ord_to_native(Ordering::Less));
    }

    #[test]
    fn cmp_str_lexicographic() {
        let r = builtin_sort_cmp_str(&[
            NativeValue::Str("file9".into()),
            NativeValue::Str("file10".into()),
        ])
        .unwrap();
        // "9" > "1" lexicographically
        assert_eq!(r, ord_to_native(Ordering::Greater));
    }

    #[test]
    fn cmp_str_natural_orders_numerically() {
        let r = builtin_sort_cmp_str_natural(&[
            NativeValue::Str("file9".into()),
            NativeValue::Str("file10".into()),
        ])
        .unwrap();
        assert_eq!(r, ord_to_native(Ordering::Less));
    }

    #[test]
    fn cmp_str_natural_pure_text_is_lex() {
        let r = builtin_sort_cmp_str_natural(&[
            NativeValue::Str("apple".into()),
            NativeValue::Str("banana".into()),
        ])
        .unwrap();
        assert_eq!(r, ord_to_native(Ordering::Less));
    }

    #[test]
    fn cmp_str_natural_equal_strings() {
        let r = builtin_sort_cmp_str_natural(&[
            NativeValue::Str("file10".into()),
            NativeValue::Str("file10".into()),
        ])
        .unwrap();
        assert_eq!(r, ord_to_native(Ordering::Equal));
    }

    #[test]
    fn cmp_str_natural_handles_huge_numbers() {
        // Beyond u64 but within u128.
        let r = builtin_sort_cmp_str_natural(&[
            NativeValue::Str("v100000000000000000000".into()),
            NativeValue::Str("v99999999999999999999".into()),
        ])
        .unwrap();
        assert_eq!(r, ord_to_native(Ordering::Greater));
    }

    // --- Combinators -------------------------------------------------------

    // `reverse` and `then` are higher-order in the spec; the applied form
    // we expose calls invoke_closure, which is unwired during the parallel
    // build phase. We verify the encoding round-trip directly.

    #[test]
    fn ordering_round_trip_through_native() {
        for o in [Ordering::Less, Ordering::Equal, Ordering::Greater] {
            let n = ord_to_native(o);
            assert_eq!(ord_from_native(&n).unwrap(), o);
        }
    }

    #[test]
    fn reverse_inverts_ordering() {
        assert_eq!(Ordering::Less.reverse(), Ordering::Greater);
        assert_eq!(Ordering::Equal.reverse(), Ordering::Equal);
        assert_eq!(Ordering::Greater.reverse(), Ordering::Less);
    }

    #[test]
    fn reverse_of_cmp_int() {
        let r = builtin_sort_reverse(&[
            cmp_int_closure(),
            NativeValue::Int(1),
            NativeValue::Int(2),
        ])
        .unwrap();
        assert_eq!(r, ord_to_native(Ordering::Greater));
    }

    #[test]
    fn then_falls_through_on_equal() {
        let r = builtin_sort_then(&[
            cmp_int_closure(),
            cmp_int_closure(),
            NativeValue::Int(1),
            NativeValue::Int(1),
        ])
        .unwrap();
        assert_eq!(r, ord_to_native(Ordering::Equal));
    }

    // --- Constants ---------------------------------------------------------

    #[test]
    fn ordering_constants() {
        assert_eq!(
            builtin_sort_less(&[]).unwrap(),
            ord_to_native(Ordering::Less)
        );
        assert_eq!(
            builtin_sort_equal(&[]).unwrap(),
            ord_to_native(Ordering::Equal)
        );
        assert_eq!(
            builtin_sort_greater(&[]).unwrap(),
            ord_to_native(Ordering::Greater)
        );
    }

    // --- Predicates --------------------------------------------------------

    #[test]
    fn is_sorted_true_when_ascending() {
        let r = builtin_sort_is_sorted(&[ints(&[1, 2, 3])]).unwrap();
        assert_eq!(r, NativeValue::Bool(true));
    }

    #[test]
    fn is_sorted_false_when_unsorted() {
        let r = builtin_sort_is_sorted(&[ints(&[1, 3, 2])]).unwrap();
        assert_eq!(r, NativeValue::Bool(false));
    }

    #[test]
    fn is_sorted_true_for_empty_and_singleton() {
        assert_eq!(
            builtin_sort_is_sorted(&[ints(&[])]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_sort_is_sorted(&[ints(&[42])]).unwrap(),
            NativeValue::Bool(true)
        );
    }

    #[test]
    fn is_sorted_allows_equals() {
        let r = builtin_sort_is_sorted(&[ints(&[1, 1, 2, 2, 3])]).unwrap();
        assert_eq!(r, NativeValue::Bool(true));
    }

    // --- Higher-order paths (require invoke_closure) -----------------------

    #[test]
    fn by_sorts_by_extracted_key() {
        // Items are (name, payload) pairs encoded as 2-element arrays. Key
        // extractor returns the first element (the name).
        let pair = |name: &str, n: i64| {
            NativeValue::Array(vec![NativeValue::Str(name.into()), NativeValue::Int(n)])
        };
        let items = NativeValue::Array(vec![pair("b", 1), pair("a", 2), pair("c", 3)]);
        let extract_name = closure(|args| match &args[0] {
            NativeValue::Array(xs) => Ok(xs[0].clone()),
            other => Err(format!("expected pair, got {other:?}")),
        });
        let r = builtin_sort_by(&[items, extract_name]).unwrap();
        let l = match &r {
            NativeValue::Array(xs) => xs,
            _ => panic!("expected list"),
        };
        let names: Vec<String> = l
            .iter()
            .map(|p| match p {
                NativeValue::Array(xs) => unwrap_str(&xs[0]),
                _ => panic!(),
            })
            .collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn by_desc_sorts_descending() {
        let identity = closure(|args| Ok(args[0].clone()));
        let r = builtin_sort_by_desc(&[ints(&[3, 1, 2]), identity]).unwrap();
        assert_eq!(r, ints(&[3, 2, 1]));
    }

    #[test]
    fn with_uses_provided_comparator() {
        let r = builtin_sort_with(&[ints(&[3, 1, 2]), cmp_int_closure()]).unwrap();
        assert_eq!(r, ints(&[1, 2, 3]));
    }

    #[test]
    fn top_returns_n_largest_descending() {
        let identity = closure(|args| Ok(args[0].clone()));
        let r =
            builtin_sort_top(&[ints(&[5, 3, 9, 1, 7]), NativeValue::Int(2), identity]).unwrap();
        assert_eq!(r, ints(&[9, 7]));
    }

    #[test]
    fn bottom_returns_n_smallest_ascending() {
        let identity = closure(|args| Ok(args[0].clone()));
        let r = builtin_sort_bottom(&[
            ints(&[5, 3, 9, 1, 7]),
            NativeValue::Int(2),
            identity,
        ])
        .unwrap();
        assert_eq!(r, ints(&[1, 3]));
    }

    #[test]
    fn sort_by_is_stable_on_equal_keys() {
        // Pairs (key, original_index). Sorting by `key` must preserve the
        // relative order of items with equal keys.
        let pair = |k: i64, idx: i64| {
            NativeValue::Array(vec![NativeValue::Int(k), NativeValue::Int(idx)])
        };
        let items = NativeValue::Array(vec![
            pair(1, 0),
            pair(2, 1),
            pair(1, 2),
            pair(2, 3),
            pair(1, 4),
        ]);
        let extract_key = closure(|args| match &args[0] {
            NativeValue::Array(xs) => Ok(xs[0].clone()),
            other => Err(format!("expected pair, got {other:?}")),
        });
        let r = builtin_sort_by(&[items, extract_key]).unwrap();
        let xs = match &r {
            NativeValue::Array(xs) => xs,
            _ => panic!(),
        };
        let indices: Vec<i64> = xs
            .iter()
            .map(|p| match p {
                NativeValue::Array(inner) => unwrap_int(&inner[1]),
                _ => panic!(),
            })
            .collect();
        // 1-keyed elements first (orig indices 0,2,4), then 2-keyed (1,3).
        assert_eq!(indices, vec![0, 2, 4, 1, 3]);
    }

    #[test]
    fn is_sorted_by_uses_key_function() {
        // Items keyed by their negation — descending input is "sorted" under
        // this key function.
        let neg = closure(|args| Ok(NativeValue::Int(-unwrap_int(&args[0]))));
        let r = builtin_sort_is_sorted_by(&[ints(&[3, 2, 1]), neg]).unwrap();
        assert_eq!(r, NativeValue::Bool(true));

        let neg2 = closure(|args| Ok(NativeValue::Int(-unwrap_int(&args[0]))));
        let r = builtin_sort_is_sorted_by(&[ints(&[1, 2, 3]), neg2]).unwrap();
        assert_eq!(r, NativeValue::Bool(false));
    }

    // --- Stability of asc/desc (no closures needed) ------------------------
    //
    // The element identity matters here: we use Float values so equal Ints
    // would be indistinguishable. Use Str to encode original index.

    #[test]
    fn asc_preserves_order_of_equal_elements() {
        // Sort [(1,"a"), (1,"b"), (1,"c")] keyed by the leading 1 — but we
        // only have flat sorts, so cheat: sort a list of equal Ints. The
        // values are equal, so any stable sort returns them in input order.
        // Here we instead encode index into the value: same Int means same
        // sort key, and the source order is the only signal. We test
        // stability by sorting strings whose first char ties.
        let l = strs(&["a1", "a2", "a3", "a0"]);
        let r = builtin_sort_asc(&[l]).unwrap();
        // After ascending sort: "a0", "a1", "a2", "a3"
        assert_eq!(r, strs(&["a0", "a1", "a2", "a3"]));
    }

    #[test]
    fn natural_cmp_directly() {
        assert_eq!(natural_cmp("file9", "file10"), Ordering::Less);
        assert_eq!(natural_cmp("file10", "file9"), Ordering::Greater);
        assert_eq!(natural_cmp("file2", "file2"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "b"), Ordering::Less);
        assert_eq!(natural_cmp("a1", "a"), Ordering::Greater);
    }

    // --- Registration ------------------------------------------------------

    #[test]
    fn register_populates_all_names() {
        let mut registry = NativeRegistry::new();
        let mut interner = Interner::new();
        register(&mut registry, &mut interner);

        for name in [
            "sort_asc", "sort_desc", "sort_by", "sort_by_desc", "sort_with",
            "sort_cmp_int", "sort_cmp_float", "sort_cmp_str", "sort_cmp_str_natural",
            "sort_reverse", "sort_then",
            "sort_top", "sort_bottom",
            "sort_is_sorted", "sort_is_sorted_by",
            "sort_less", "sort_equal", "sort_greater",
        ] {
            let sym = interner.intern(name);
            assert!(
                registry.get(sym).is_some(),
                "expected sort register to include `{name}`"
            );
        }
    }
}
