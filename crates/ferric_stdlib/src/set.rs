//! # `set` — Hash Sets
//!
//! Immutable set operations. Functions follow the `set_*` native naming
//! convention (see `docs/stdlib.md`).
//!
//! Elements are restricted to `Int`, `Str`, and `Bool` via `MapKey` — the
//! same restriction as `map` keys. `Float` elements are forbidden because of
//! `NaN`. Storage uses `BTreeSet`; the integration task may swap to
//! `HashSet` once `NativeValue` gains `Eq + Hash`.

// REQUIRED NATIVE VALUE VARIANTS:
//   Set(BTreeSet<MapKey>)
//   Map(BTreeMap<MapKey, NativeValue>)         // shared with map.rs
//   List(Vec<NativeValue>)                     // alias for the existing Array variant
//
// MapKey is defined in `map.rs`. Float elements are forbidden (NaN). The
// integration task may upgrade to HashSet once NativeValue gains Eq + Hash.

// REQUIRED DEPS:
//   none — std::collections::BTreeSet

use std::collections::BTreeSet;

use ferric_common::{Interner, Symbol};

use crate::{invoke_closure, MapKey, NativeRegistry, NativeValue};

// ============================================================================
// Helpers (private to this module)
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

fn expect_set(v: &NativeValue) -> Result<&BTreeSet<MapKey>, String> {
    match v {
        NativeValue::Set(s) => Ok(s),
        other => Err(format!("expected set, got {:?}", other)),
    }
}

fn expect_list(v: &NativeValue) -> Result<&Vec<NativeValue>, String> {
    match v {
        NativeValue::List(xs) => Ok(xs),
        other => Err(format!("expected list, got {:?}", other)),
    }
}

// ============================================================================
// Construction
// ============================================================================

/// `set::new() -> Set`.
fn builtin_set_new(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::Set(BTreeSet::new()))
}

/// `set::from_list(l) -> Set`. Duplicates collapse.
fn builtin_set_from_list(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let xs = expect_list(&args[0])?;
    let mut out = BTreeSet::new();
    for x in xs {
        out.insert(MapKey::from_value(x)?);
    }
    Ok(NativeValue::Set(out))
}

// ============================================================================
// Membership
// ============================================================================

/// `set::contains(s, item) -> Bool`.
fn builtin_set_contains(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_set(&args[0])?;
    let k = MapKey::from_value(&args[1])?;
    Ok(NativeValue::Bool(s.contains(&k)))
}

/// `set::len(s) -> Int`.
fn builtin_set_len(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_set(&args[0])?;
    Ok(NativeValue::Int(s.len() as i64))
}

/// `set::is_empty(s) -> Bool`.
fn builtin_set_is_empty(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_set(&args[0])?;
    Ok(NativeValue::Bool(s.is_empty()))
}

// ============================================================================
// Modification — return a new set
// ============================================================================

/// `set::insert(s, item) -> Set`. No-op if already present.
fn builtin_set_insert(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_set(&args[0])?;
    let k = MapKey::from_value(&args[1])?;
    let mut next = s.clone();
    next.insert(k);
    Ok(NativeValue::Set(next))
}

/// `set::remove(s, item) -> Set`. No-op if absent.
fn builtin_set_remove(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_set(&args[0])?;
    let k = MapKey::from_value(&args[1])?;
    let mut next = s.clone();
    next.remove(&k);
    Ok(NativeValue::Set(next))
}

// ============================================================================
// Set algebra
// ============================================================================

/// `set::union(a, b) -> Set`.
fn builtin_set_union(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_set(&args[0])?;
    let b = expect_set(&args[1])?;
    Ok(NativeValue::Set(a.union(b).cloned().collect()))
}

/// `set::intersection(a, b) -> Set`.
fn builtin_set_intersection(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_set(&args[0])?;
    let b = expect_set(&args[1])?;
    Ok(NativeValue::Set(a.intersection(b).cloned().collect()))
}

/// `set::difference(a, b) -> Set` — items in `a` but not in `b`.
fn builtin_set_difference(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_set(&args[0])?;
    let b = expect_set(&args[1])?;
    Ok(NativeValue::Set(a.difference(b).cloned().collect()))
}

/// `set::symmetric_difference(a, b) -> Set`.
fn builtin_set_symmetric_difference(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_set(&args[0])?;
    let b = expect_set(&args[1])?;
    Ok(NativeValue::Set(
        a.symmetric_difference(b).cloned().collect(),
    ))
}

/// `set::is_subset(a, b) -> Bool` — every element of `a` is also in `b`.
fn builtin_set_is_subset(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_set(&args[0])?;
    let b = expect_set(&args[1])?;
    Ok(NativeValue::Bool(a.is_subset(b)))
}

/// `set::is_superset(a, b) -> Bool`.
fn builtin_set_is_superset(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_set(&args[0])?;
    let b = expect_set(&args[1])?;
    Ok(NativeValue::Bool(a.is_superset(b)))
}

/// `set::is_disjoint(a, b) -> Bool` — no element in common.
fn builtin_set_is_disjoint(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_set(&args[0])?;
    let b = expect_set(&args[1])?;
    Ok(NativeValue::Bool(a.is_disjoint(b)))
}

// ============================================================================
// Convert
// ============================================================================

/// `set::to_list(s) -> List`. Order is `BTreeSet`'s natural order until the
/// representation swaps to a hash set.
fn builtin_set_to_list(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_set(&args[0])?;
    Ok(NativeValue::List(s.iter().map(|k| k.to_value()).collect()))
}

// ============================================================================
// Transform
// ============================================================================

/// `set::filter(s, f) -> Set`. Keeps elements where `f(item)` is true.
fn builtin_set_filter(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_set(&args[0])?;
    let f = &args[1];
    let mut out = BTreeSet::new();
    for k in s.iter() {
        let keep = invoke_closure(f, &[k.to_value()])?;
        if matches!(keep, NativeValue::Bool(true)) {
            out.insert(k.clone());
        }
    }
    Ok(NativeValue::Set(out))
}

/// `set::map(s, f) -> Set`. Applies `f(item)` to each element. The result
/// must still be a valid `MapKey` (Int/Str/Bool).
fn builtin_set_map(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_set(&args[0])?;
    let f = &args[1];
    let mut out = BTreeSet::new();
    for k in s.iter() {
        let mapped = invoke_closure(f, &[k.to_value()])?;
        out.insert(MapKey::from_value(&mapped)?);
    }
    Ok(NativeValue::Set(out))
}

// ============================================================================
// Registration
// ============================================================================

/// Registers every `set_*` native with the runtime registry.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    let mut go = |name: &str, f: fn(&[NativeValue]) -> Result<NativeValue, String>| {
        let sym: Symbol = interner.intern(name);
        registry.register(sym, f);
    };

    go("set_new", builtin_set_new);
    go("set_from_list", builtin_set_from_list);
    go("set_contains", builtin_set_contains);
    go("set_len", builtin_set_len);
    go("set_is_empty", builtin_set_is_empty);
    go("set_insert", builtin_set_insert);
    go("set_remove", builtin_set_remove);
    go("set_union", builtin_set_union);
    go("set_intersection", builtin_set_intersection);
    go("set_difference", builtin_set_difference);
    go("set_symmetric_difference", builtin_set_symmetric_difference);
    go("set_is_subset", builtin_set_is_subset);
    go("set_is_superset", builtin_set_is_superset);
    go("set_is_disjoint", builtin_set_is_disjoint);
    go("set_to_list", builtin_set_to_list);
    go("set_filter", builtin_set_filter);
    go("set_map", builtin_set_map);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn i(n: i64) -> NativeValue {
        NativeValue::Int(n)
    }

    fn list(items: Vec<NativeValue>) -> NativeValue {
        NativeValue::List(items)
    }

    fn set_of(items: Vec<i64>) -> NativeValue {
        builtin_set_from_list(&[list(items.into_iter().map(i).collect())]).unwrap()
    }

    fn ints_in_set(s: &NativeValue) -> Vec<i64> {
        match s {
            NativeValue::Set(inner) => inner
                .iter()
                .map(|k| match k {
                    MapKey::Int(n) => *n,
                    _ => panic!("expected Int key"),
                })
                .collect(),
            _ => panic!("not a set"),
        }
    }

    // ---- from_list / len -------------------------------------------------

    #[test]
    fn from_list_dedupes() {
        let s = set_of(vec![1, 2, 2, 3]);
        let len = builtin_set_len(&[s]).unwrap();
        assert_eq!(len, NativeValue::Int(3));
    }

    #[test]
    fn empty_set_is_empty() {
        let s = builtin_set_new(&[]).unwrap();
        assert_eq!(
            builtin_set_is_empty(&[s]).unwrap(),
            NativeValue::Bool(true)
        );
    }

    #[test]
    fn nonempty_set_not_empty() {
        let s = set_of(vec![1]);
        assert_eq!(
            builtin_set_is_empty(&[s]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    // ---- contains --------------------------------------------------------

    #[test]
    fn contains_true_false() {
        let s = set_of(vec![1, 2, 3]);
        assert_eq!(
            builtin_set_contains(&[s.clone(), i(2)]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_set_contains(&[s, i(99)]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    // ---- insert / remove -------------------------------------------------

    #[test]
    fn insert_existing_is_noop() {
        let s = set_of(vec![1, 2, 3]);
        let after = builtin_set_insert(&[s, i(1)]).unwrap();
        assert_eq!(builtin_set_len(&[after]).unwrap(), i(3));
    }

    #[test]
    fn insert_new_grows() {
        let s = set_of(vec![1, 2, 3]);
        let after = builtin_set_insert(&[s, i(4)]).unwrap();
        assert_eq!(builtin_set_len(&[after]).unwrap(), i(4));
    }

    #[test]
    fn remove_present_shrinks() {
        let s = set_of(vec![1, 2, 3]);
        let after = builtin_set_remove(&[s, i(2)]).unwrap();
        assert_eq!(builtin_set_len(&[after.clone()]).unwrap(), i(2));
        assert_eq!(
            builtin_set_contains(&[after, i(2)]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn remove_absent_is_noop() {
        let s = set_of(vec![1, 2, 3]);
        let after = builtin_set_remove(&[s, i(99)]).unwrap();
        assert_eq!(builtin_set_len(&[after]).unwrap(), i(3));
    }

    // ---- set algebra -----------------------------------------------------

    #[test]
    fn union_combines() {
        let a = set_of(vec![1, 2]);
        let b = set_of(vec![2, 3]);
        let u = builtin_set_union(&[a, b]).unwrap();
        let mut got = ints_in_set(&u);
        got.sort();
        assert_eq!(got, vec![1, 2, 3]);
    }

    #[test]
    fn intersection_overlap() {
        let a = set_of(vec![1, 2]);
        let b = set_of(vec![2, 3]);
        let inter = builtin_set_intersection(&[a, b]).unwrap();
        assert_eq!(ints_in_set(&inter), vec![2]);
    }

    #[test]
    fn difference_subtracts() {
        let a = set_of(vec![1, 2]);
        let b = set_of(vec![2, 3]);
        let d = builtin_set_difference(&[a, b]).unwrap();
        assert_eq!(ints_in_set(&d), vec![1]);
    }

    #[test]
    fn symmetric_difference_xor() {
        let a = set_of(vec![1, 2]);
        let b = set_of(vec![2, 3]);
        let sd = builtin_set_symmetric_difference(&[a, b]).unwrap();
        let mut got = ints_in_set(&sd);
        got.sort();
        assert_eq!(got, vec![1, 3]);
    }

    // ---- subset / superset / disjoint ------------------------------------

    #[test]
    fn is_subset_true() {
        let a = set_of(vec![1, 2]);
        let b = set_of(vec![1, 2, 3]);
        assert_eq!(
            builtin_set_is_subset(&[a, b]).unwrap(),
            NativeValue::Bool(true)
        );
    }

    #[test]
    fn is_subset_false() {
        let a = set_of(vec![1, 4]);
        let b = set_of(vec![1, 2, 3]);
        assert_eq!(
            builtin_set_is_subset(&[a, b]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn is_superset_true() {
        let a = set_of(vec![1, 2, 3]);
        let b = set_of(vec![1, 2]);
        assert_eq!(
            builtin_set_is_superset(&[a, b]).unwrap(),
            NativeValue::Bool(true)
        );
    }

    #[test]
    fn is_disjoint_true() {
        let a = set_of(vec![1, 2]);
        let b = set_of(vec![3, 4]);
        assert_eq!(
            builtin_set_is_disjoint(&[a, b]).unwrap(),
            NativeValue::Bool(true)
        );
    }

    #[test]
    fn is_disjoint_false() {
        let a = set_of(vec![1, 2]);
        let b = set_of(vec![2, 3]);
        assert_eq!(
            builtin_set_is_disjoint(&[a, b]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    // ---- to_list ---------------------------------------------------------

    #[test]
    fn to_list_length_matches_len() {
        let s = set_of(vec![5, 1, 3]);
        let len = match builtin_set_len(&[s.clone()]).unwrap() {
            NativeValue::Int(n) => n,
            _ => panic!(),
        };
        let l = builtin_set_to_list(&[s]).unwrap();
        match l {
            NativeValue::List(xs) => assert_eq!(xs.len() as i64, len),
            _ => panic!("not a list"),
        }
    }

    // ---- key validation --------------------------------------------------

    #[test]
    fn float_element_is_rejected() {
        let res = builtin_set_from_list(&[list(vec![NativeValue::Float(1.5)])]);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Float"));
    }

    // filter / map require invoke_closure plumbing — covered in integration.
}
