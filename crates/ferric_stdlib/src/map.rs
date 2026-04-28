//! # `map` — Hash Maps
//!
//! Immutable map operations plus a mutable builder. Functions follow the
//! `map_*` native naming convention (see `docs/stdlib.md`).
//!
//! Keys are restricted to `Int`, `Str`, and `Bool` via the private `MapKey`
//! enum. `Float` keys are forbidden because of `NaN`. Storage uses `BTreeMap`
//! for now; the integration task may swap to `HashMap` once `NativeValue`
//! gains `Eq + Hash`.

// REQUIRED NATIVE VALUE VARIANTS:
//   Map(BTreeMap<MapKey, NativeValue>)
//   MapMut(Rc<RefCell<BTreeMap<MapKey, NativeValue>>>)
//   Set(BTreeSet<MapKey>)
//   Tuple2(Box<NativeValue>, Box<NativeValue>)
//   List(Vec<NativeValue>)            // alias for the existing Array variant
//
// MapKey is a hashable+orderable subset of NativeValue:
//   pub enum MapKey { Int(i64), Str(String), Bool(bool) }
// Float keys are forbidden (NaN). Integration may upgrade to HashMap once
// NativeValue gains Eq + Hash.

// REQUIRED DEPS:
//   none — std::collections::BTreeMap

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use ferric_common::{Interner, Symbol};

use crate::{MapKey, NativeRegistry, NativeValue};

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

fn expect_int(v: &NativeValue) -> Result<i64, String> {
    match v {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("expected int, got {other:?}")),
    }
}

fn expect_list(v: &NativeValue) -> Result<&Vec<NativeValue>, String> {
    match v {
        NativeValue::List(xs) => Ok(xs),
        other => Err(format!("expected list, got {other:?}")),
    }
}

fn expect_map(v: &NativeValue) -> Result<&BTreeMap<MapKey, NativeValue>, String> {
    match v {
        NativeValue::Map(m) => Ok(m),
        other => Err(format!("expected map, got {other:?}")),
    }
}

fn expect_map_mut(
    v: &NativeValue,
) -> Result<&Rc<RefCell<BTreeMap<MapKey, NativeValue>>>, String> {
    match v {
        NativeValue::MapMut(m) => Ok(m),
        other => Err(format!("expected map builder, got {other:?}")),
    }
}

fn expect_tuple2(v: &NativeValue) -> Result<(&NativeValue, &NativeValue), String> {
    match v {
        NativeValue::Tuple2(a, b) => Ok((a.as_ref(), b.as_ref())),
        other => Err(format!("expected (key, value) tuple, got {other:?}")),
    }
}

fn make_pair(k: MapKey, v: NativeValue) -> NativeValue {
    NativeValue::Tuple2(Box::new(k.into_value()), Box::new(v))
}

/// Closure invocation — assumed to exist on the parent crate
/// (`crate::invoke_closure`). Higher-order natives below use this; the
/// integration task wires it up to the VM.
use crate::invoke_closure;

// ============================================================================
// Construction
// ============================================================================

/// `map::new() -> Map`
fn builtin_map_new(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::Map(BTreeMap::new()))
}

/// `map::from_list(pairs: List<(K, V)>) -> Map`. Last write wins on duplicate keys.
fn builtin_map_from_list(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let pairs = expect_list(&args[0])?;
    let mut out = BTreeMap::new();
    for p in pairs {
        let (k, v) = expect_tuple2(p)?;
        out.insert(MapKey::from_value(k)?, v.clone());
    }
    Ok(NativeValue::Map(out))
}

/// `map::from_lists(keys: List<K>, vals: List<V>) -> Result<Map, MapError>`.
/// Errors on length mismatch.
fn builtin_map_from_lists(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let keys = expect_list(&args[0])?;
    let vals = expect_list(&args[1])?;
    if keys.len() != vals.len() {
        return Err(format!(
            "MapError::LengthMismatch: keys={} vals={}",
            keys.len(),
            vals.len()
        ));
    }
    let mut out = BTreeMap::new();
    for (k, v) in keys.iter().zip(vals.iter()) {
        out.insert(MapKey::from_value(k)?, v.clone());
    }
    Ok(NativeValue::Map(out))
}

// ============================================================================
// Access
// ============================================================================

/// `map::get(m, key) -> Option<V>`. Returned as either the value or `Unit`
/// (Option plumbing is handled by integration / Ferric `Option` lowering).
fn builtin_map_get(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let m = expect_map(&args[0])?;
    let k = MapKey::from_value(&args[1])?;
    Ok(match m.get(&k) {
        Some(v) => v.clone(),
        None => NativeValue::Unit,
    })
}

/// `map::get_or(m, key, default) -> V`.
fn builtin_map_get_or(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let m = expect_map(&args[0])?;
    let k = MapKey::from_value(&args[1])?;
    Ok(m.get(&k).cloned().unwrap_or_else(|| args[2].clone()))
}

/// `map::contains_key(m, key) -> Bool`.
fn builtin_map_contains_key(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let m = expect_map(&args[0])?;
    let k = MapKey::from_value(&args[1])?;
    Ok(NativeValue::Bool(m.contains_key(&k)))
}

/// `map::len(m) -> Int`.
fn builtin_map_len(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let m = expect_map(&args[0])?;
    Ok(NativeValue::Int(m.len() as i64))
}

/// `map::is_empty(m) -> Bool`.
fn builtin_map_is_empty(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let m = expect_map(&args[0])?;
    Ok(NativeValue::Bool(m.is_empty()))
}

// ============================================================================
// Modification — return a new map
// ============================================================================

/// `map::insert(m, key, val) -> Map`.
fn builtin_map_insert(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let m = expect_map(&args[0])?;
    let k = MapKey::from_value(&args[1])?;
    let mut next = m.clone();
    next.insert(k, args[2].clone());
    Ok(NativeValue::Map(next))
}

/// `map::remove(m, key) -> Map`. No-op if `key` is absent.
fn builtin_map_remove(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let m = expect_map(&args[0])?;
    let k = MapKey::from_value(&args[1])?;
    let mut next = m.clone();
    next.remove(&k);
    Ok(NativeValue::Map(next))
}

/// `map::merge(a, b) -> Map`. `b` wins on conflict.
fn builtin_map_merge(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_map(&args[0])?;
    let b = expect_map(&args[1])?;
    let mut out = a.clone();
    for (k, v) in b.iter() {
        out.insert(k.clone(), v.clone());
    }
    Ok(NativeValue::Map(out))
}

/// `map::merge_with(a, b, f) -> Map`. On conflict, value is `f(a_val, b_val)`.
fn builtin_map_merge_with(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let a = expect_map(&args[0])?;
    let b = expect_map(&args[1])?;
    let f = &args[2];
    let mut out = a.clone();
    for (k, bv) in b.iter() {
        let merged = match out.get(k) {
            Some(av) => invoke_closure(f, &[av.clone(), bv.clone()])?,
            None => bv.clone(),
        };
        out.insert(k.clone(), merged);
    }
    Ok(NativeValue::Map(out))
}

// ============================================================================
// Views
// ============================================================================

/// `map::keys(m) -> List<K>`.
fn builtin_map_keys(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let m = expect_map(&args[0])?;
    Ok(NativeValue::List(m.keys().map(|k| k.to_value()).collect()))
}

/// `map::values(m) -> List<V>`.
fn builtin_map_values(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let m = expect_map(&args[0])?;
    Ok(NativeValue::List(m.values().cloned().collect()))
}

/// `map::entries(m) -> List<(K, V)>`.
fn builtin_map_entries(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let m = expect_map(&args[0])?;
    let entries = m
        .iter()
        .map(|(k, v)| make_pair(k.clone(), v.clone()))
        .collect();
    Ok(NativeValue::List(entries))
}

// ============================================================================
// Transform
// ============================================================================

/// `map::map_values(m, f) -> Map`. Applies `f(v)` to every value.
fn builtin_map_map_values(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let m = expect_map(&args[0])?;
    let f = &args[1];
    let mut out = BTreeMap::new();
    for (k, v) in m.iter() {
        let next = invoke_closure(f, &[v.clone()])?;
        out.insert(k.clone(), next);
    }
    Ok(NativeValue::Map(out))
}

/// `map::filter(m, f) -> Map`. Keeps entries where `f(k, v)` is true.
fn builtin_map_filter(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let m = expect_map(&args[0])?;
    let f = &args[1];
    let mut out = BTreeMap::new();
    for (k, v) in m.iter() {
        let keep = invoke_closure(f, &[k.to_value(), v.clone()])?;
        if matches!(keep, NativeValue::Bool(true)) {
            out.insert(k.clone(), v.clone());
        }
    }
    Ok(NativeValue::Map(out))
}

/// `map::filter_map(m, f) -> Map<K, W>`. `f` returns `Some(w)` to keep.
/// Without a real `Option` ABI here, treat `Unit` as `None` and any other
/// value as `Some(value)`.
fn builtin_map_filter_map(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let m = expect_map(&args[0])?;
    let f = &args[1];
    let mut out = BTreeMap::new();
    for (k, v) in m.iter() {
        let res = invoke_closure(f, &[k.to_value(), v.clone()])?;
        if !matches!(res, NativeValue::Unit) {
            out.insert(k.clone(), res);
        }
    }
    Ok(NativeValue::Map(out))
}

/// `map::fold(m, init, f) -> U` where `f(acc, k, v) -> acc'`.
fn builtin_map_fold(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let m = expect_map(&args[0])?;
    let mut acc = args[1].clone();
    let f = &args[2];
    for (k, v) in m.iter() {
        acc = invoke_closure(f, &[acc, k.to_value(), v.clone()])?;
    }
    Ok(acc)
}

// ============================================================================
// Grouping
// ============================================================================

/// `map::group_by(l, f) -> Map<K, List<T>>`.
fn builtin_map_group_by(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?;
    let f = &args[1];
    let mut out: BTreeMap<MapKey, Vec<NativeValue>> = BTreeMap::new();
    for item in l.iter() {
        let k_val = invoke_closure(f, &[item.clone()])?;
        let k = MapKey::from_value(&k_val)?;
        out.entry(k).or_default().push(item.clone());
    }
    let lifted: BTreeMap<MapKey, NativeValue> = out
        .into_iter()
        .map(|(k, v)| (k, NativeValue::List(v)))
        .collect();
    Ok(NativeValue::Map(lifted))
}

/// `map::count_by(l, f) -> Map<K, Int>`.
fn builtin_map_count_by(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?;
    let f = &args[1];
    let mut out: BTreeMap<MapKey, i64> = BTreeMap::new();
    for item in l.iter() {
        let k_val = invoke_closure(f, &[item.clone()])?;
        let k = MapKey::from_value(&k_val)?;
        *out.entry(k).or_insert(0) += 1;
    }
    let lifted: BTreeMap<MapKey, NativeValue> =
        out.into_iter().map(|(k, n)| (k, NativeValue::Int(n))).collect();
    Ok(NativeValue::Map(lifted))
}

// ============================================================================
// Mutable builder
// ============================================================================

/// `map::builder() -> MapMut`.
fn builtin_map_builder(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::MapMut(Rc::new(RefCell::new(BTreeMap::new()))))
}

/// `map::set(buf, key, val) -> Unit`. Mutates the builder in place.
fn builtin_map_set(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let buf = expect_map_mut(&args[0])?;
    let k = MapKey::from_value(&args[1])?;
    buf.borrow_mut().insert(k, args[2].clone());
    Ok(NativeValue::Unit)
}

/// `map::build(buf) -> Map`. Snapshots the builder into an immutable map.
fn builtin_map_build(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let buf = expect_map_mut(&args[0])?;
    Ok(NativeValue::Map(buf.borrow().clone()))
}

// ============================================================================
// Registration
// ============================================================================

/// Registers every `map_*` native with the runtime registry.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    let mut go = |name: &str, f: fn(&[NativeValue]) -> Result<NativeValue, String>| {
        let sym: Symbol = interner.intern(name);
        registry.register(sym, f);
    };

    go("map_new", builtin_map_new);
    go("map_from_list", builtin_map_from_list);
    go("map_from_lists", builtin_map_from_lists);
    go("map_get", builtin_map_get);
    go("map_get_or", builtin_map_get_or);
    go("map_contains_key", builtin_map_contains_key);
    go("map_len", builtin_map_len);
    go("map_is_empty", builtin_map_is_empty);
    go("map_insert", builtin_map_insert);
    go("map_remove", builtin_map_remove);
    go("map_merge", builtin_map_merge);
    go("map_merge_with", builtin_map_merge_with);
    go("map_keys", builtin_map_keys);
    go("map_values", builtin_map_values);
    go("map_entries", builtin_map_entries);
    go("map_map_values", builtin_map_map_values);
    go("map_filter", builtin_map_filter);
    go("map_filter_map", builtin_map_filter_map);
    go("map_fold", builtin_map_fold);
    go("map_group_by", builtin_map_group_by);
    go("map_count_by", builtin_map_count_by);
    go("map_builder", builtin_map_builder);
    go("map_set", builtin_map_set);
    go("map_build", builtin_map_build);
    let _ = expect_int; // silence unused warning until used by future helpers
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn k_str(s: &str) -> MapKey {
        MapKey::Str(s.to_string())
    }

    fn pair(k: NativeValue, v: NativeValue) -> NativeValue {
        NativeValue::Tuple2(Box::new(k), Box::new(v))
    }

    fn list(items: Vec<NativeValue>) -> NativeValue {
        NativeValue::List(items)
    }

    fn s(x: &str) -> NativeValue {
        NativeValue::Str(x.to_string())
    }

    fn i(n: i64) -> NativeValue {
        NativeValue::Int(n)
    }

    // ---- from_list -------------------------------------------------------

    #[test]
    fn from_list_basic() {
        let pairs = list(vec![pair(s("a"), i(1)), pair(s("b"), i(2))]);
        let m = builtin_map_from_list(&[pairs]).unwrap();
        let len = builtin_map_len(&[m.clone()]).unwrap();
        assert_eq!(len, NativeValue::Int(2));
        let got = builtin_map_get(&[m.clone(), s("a")]).unwrap();
        assert_eq!(got, NativeValue::Int(1));
        let missing = builtin_map_get(&[m, s("z")]).unwrap();
        assert_eq!(missing, NativeValue::Unit);
    }

    #[test]
    fn from_list_last_wins_on_dup() {
        let pairs = list(vec![pair(s("k"), i(1)), pair(s("k"), i(99))]);
        let m = builtin_map_from_list(&[pairs]).unwrap();
        let v = builtin_map_get(&[m, s("k")]).unwrap();
        assert_eq!(v, NativeValue::Int(99));
    }

    // ---- from_lists ------------------------------------------------------

    #[test]
    fn from_lists_ok() {
        let m = builtin_map_from_lists(&[
            list(vec![s("a"), s("b")]),
            list(vec![i(1), i(2)]),
        ])
        .unwrap();
        let v = builtin_map_get(&[m, s("b")]).unwrap();
        assert_eq!(v, NativeValue::Int(2));
    }

    #[test]
    fn from_lists_length_mismatch_errs() {
        let res = builtin_map_from_lists(&[list(vec![s("a")]), list(vec![i(1), i(2)])]);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("LengthMismatch"));
    }

    // ---- insert / remove -------------------------------------------------

    #[test]
    fn insert_returns_new_map_original_unchanged() {
        let original =
            builtin_map_from_list(&[list(vec![pair(s("a"), i(1))])]).unwrap();
        let updated =
            builtin_map_insert(&[original.clone(), s("b"), i(2)]).unwrap();
        assert_eq!(builtin_map_len(&[original]).unwrap(), i(1));
        assert_eq!(builtin_map_len(&[updated]).unwrap(), i(2));
    }

    #[test]
    fn remove_noop_on_absent_key() {
        let m = builtin_map_from_list(&[list(vec![pair(s("a"), i(1))])]).unwrap();
        let after = builtin_map_remove(&[m.clone(), s("nope")]).unwrap();
        assert_eq!(builtin_map_len(&[after]).unwrap(), i(1));
    }

    #[test]
    fn remove_removes_present_key() {
        let m =
            builtin_map_from_list(&[list(vec![pair(s("a"), i(1)), pair(s("b"), i(2))])])
                .unwrap();
        let after = builtin_map_remove(&[m, s("a")]).unwrap();
        assert_eq!(builtin_map_len(&[after.clone()]).unwrap(), i(1));
        assert_eq!(
            builtin_map_contains_key(&[after, s("a")]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    // ---- contains_key / len / is_empty -----------------------------------

    #[test]
    fn contains_key_true_false() {
        let m = builtin_map_from_list(&[list(vec![pair(s("a"), i(1))])]).unwrap();
        assert_eq!(
            builtin_map_contains_key(&[m.clone(), s("a")]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_map_contains_key(&[m, s("z")]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn is_empty_true_false() {
        let empty = builtin_map_new(&[]).unwrap();
        assert_eq!(
            builtin_map_is_empty(&[empty]).unwrap(),
            NativeValue::Bool(true)
        );
        let one = builtin_map_from_list(&[list(vec![pair(s("a"), i(1))])]).unwrap();
        assert_eq!(
            builtin_map_is_empty(&[one]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn get_or_returns_default() {
        let m = builtin_map_from_list(&[list(vec![pair(s("a"), i(1))])]).unwrap();
        let v = builtin_map_get_or(&[m.clone(), s("a"), i(0)]).unwrap();
        assert_eq!(v, i(1));
        let d = builtin_map_get_or(&[m, s("x"), i(99)]).unwrap();
        assert_eq!(d, i(99));
    }

    // ---- merge / merge_with ----------------------------------------------

    #[test]
    fn merge_b_wins_on_conflict() {
        let a = builtin_map_from_list(&[list(vec![pair(s("a"), i(1)), pair(s("b"), i(2))])])
            .unwrap();
        let b = builtin_map_from_list(&[list(vec![pair(s("b"), i(3)), pair(s("c"), i(4))])])
            .unwrap();
        let merged = builtin_map_merge(&[a, b]).unwrap();
        assert_eq!(builtin_map_get(&[merged.clone(), s("a")]).unwrap(), i(1));
        assert_eq!(builtin_map_get(&[merged.clone(), s("b")]).unwrap(), i(3));
        assert_eq!(builtin_map_get(&[merged, s("c")]).unwrap(), i(4));
    }

    // merge_with requires invoke_closure plumbing — covered in integration.

    // ---- keys / values / entries -----------------------------------------

    #[test]
    fn keys_values_entries() {
        let m = builtin_map_from_list(&[list(vec![pair(s("a"), i(1)), pair(s("b"), i(2))])])
            .unwrap();
        let keys = builtin_map_keys(&[m.clone()]).unwrap();
        let vals = builtin_map_values(&[m.clone()]).unwrap();
        let entries = builtin_map_entries(&[m]).unwrap();
        match keys {
            NativeValue::List(xs) => assert_eq!(xs.len(), 2),
            _ => panic!("keys not a list"),
        }
        match vals {
            NativeValue::List(xs) => assert_eq!(xs.len(), 2),
            _ => panic!("values not a list"),
        }
        match entries {
            NativeValue::List(xs) => assert_eq!(xs.len(), 2),
            _ => panic!("entries not a list"),
        }
    }

    // ---- builder ---------------------------------------------------------

    #[test]
    fn builder_set_build_roundtrip() {
        let buf = builtin_map_builder(&[]).unwrap();
        builtin_map_set(&[buf.clone(), s("a"), i(1)]).unwrap();
        builtin_map_set(&[buf.clone(), s("b"), i(2)]).unwrap();
        builtin_map_set(&[buf.clone(), s("c"), i(3)]).unwrap();
        let m = builtin_map_build(&[buf]).unwrap();
        assert_eq!(builtin_map_len(&[m.clone()]).unwrap(), i(3));
        assert_eq!(builtin_map_get(&[m, s("b")]).unwrap(), i(2));
    }

    // ---- key validation --------------------------------------------------

    #[test]
    fn float_key_is_rejected() {
        let res = MapKey::from_value(&NativeValue::Float(1.5));
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Float"));
    }

    #[test]
    fn map_key_ord_int_str_bool() {
        // Sanity: BTreeMap ordering across MapKey variants is stable.
        let mut m = BTreeMap::new();
        m.insert(MapKey::Int(1), 'a');
        m.insert(k_str("k"), 'b');
        m.insert(MapKey::Bool(true), 'c');
        assert_eq!(m.len(), 3);
    }
}
