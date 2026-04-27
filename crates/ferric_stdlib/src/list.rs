// REQUIRED NATIVE VALUE VARIANTS:
//   List(Vec<NativeValue>)         // already exists as Array — reuse it
//   ListMut(Rc<RefCell<Vec<NativeValue>>>)
//   Closure(...)                   // for callbacks (map/filter/reduce/etc.)
//     The integration task wires a `fn invoke_closure(c: &NativeValue,
//     args: &[NativeValue]) -> Result<NativeValue, String>` helper.
//   Tuple2(Box<NativeValue>, Box<NativeValue>)
//     For zip / enumerate / partition / unzip results and for `Option::Some`
//     pair payloads if needed.
//   OptionSome(Box<NativeValue>) / OptionNone
//     For `get`, `first`, `last`, `find`, `find_index`, `index_of`, `reduce`,
//     `min`, `max`. The integration task may instead reuse a pre-existing
//     enum-tagged Option representation; this file uses small helpers
//     (`option_some`, `option_none`) that the integrator can re-route.
//
// REQUIRED DEPS:
//   none — pure std

//! `list` stdlib module — dynamic array operations.
//!
//! Higher-order functions assume `invoke_closure` exists and returns a
//! `Result<NativeValue, String>` when given a closure-shaped `NativeValue`
//! and a slice of arguments. The integration step provides the real
//! implementation; tests that rely on closures are marked `#[ignore]`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::{NativeRegistry, NativeValue};
use ferric_common::Interner;

// ---------------------------------------------------------------------------
// Closure invocation hook (provided by integration step)
// ---------------------------------------------------------------------------

/// Invoke a closure-shaped `NativeValue` with the given arguments.
///
/// During the parallel build phase this is a stub that errors out. The
/// integration task replaces it (or wires the real plumbing through the VM)
/// so that `map`, `filter`, etc. can call user code.
#[allow(dead_code)]
fn invoke_closure(_c: &NativeValue, _args: &[NativeValue]) -> Result<NativeValue, String> {
    Err("invoke_closure: closure invocation is not yet wired (integration task)".to_string())
}

// ---------------------------------------------------------------------------
// Local helpers (private to this module — integration may dedupe later)
// ---------------------------------------------------------------------------

fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!(
            "list: expected {} argument(s), got {}",
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
        other => Err(format!("list: expected Int, got {:?}", other)),
    }
}

fn expect_list(v: &NativeValue) -> Result<&Vec<NativeValue>, String> {
    match v {
        NativeValue::Array(items) => Ok(items),
        other => Err(format!("list: expected List, got {:?}", other)),
    }
}

fn expect_str<'a>(v: &'a NativeValue) -> Result<&'a String, String> {
    match v {
        NativeValue::Str(s) => Ok(s),
        other => Err(format!("list: expected Str, got {:?}", other)),
    }
}

/// Bridge to whatever Option representation the integration chooses.
/// Until then, encode `None` as `Unit` and `Some(x)` as a single-element list.
/// Integrator can swap these two helpers and tests still describe the intent.
fn option_some(v: NativeValue) -> NativeValue {
    NativeValue::Array(vec![v])
}

fn option_none() -> NativeValue {
    NativeValue::Unit
}

/// Pair encoding bridge — until the dedicated `Tuple2` variant exists,
/// represent (a, b) as a 2-element list.
fn pair(a: NativeValue, b: NativeValue) -> NativeValue {
    NativeValue::Array(vec![a, b])
}

/// Structural equality on the scalar subset we support pre-traits.
fn values_equal(a: &NativeValue, b: &NativeValue) -> bool {
    match (a, b) {
        (NativeValue::Int(x), NativeValue::Int(y)) => x == y,
        (NativeValue::Float(x), NativeValue::Float(y)) => x == y,
        (NativeValue::Bool(x), NativeValue::Bool(y)) => x == y,
        (NativeValue::Str(x), NativeValue::Str(y)) => x == y,
        (NativeValue::Unit, NativeValue::Unit) => true,
        _ => false,
    }
}

/// Truthiness for filter/take_while/etc. callbacks. Strict — only `Bool`.
fn is_truthy(v: &NativeValue) -> Result<bool, String> {
    match v {
        NativeValue::Bool(b) => Ok(*b),
        other => Err(format!("list: expected Bool from predicate, got {:?}", other)),
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

fn builtin_list_new(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::Array(Vec::new()))
}

fn builtin_list_of(args: &[NativeValue]) -> Result<NativeValue, String> {
    Ok(NativeValue::Array(args.to_vec()))
}

fn builtin_list_repeat(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let item = args[0].clone();
    let n = expect_int(&args[1])?;
    if n < 0 {
        return Err(format!("list::repeat: negative count {}", n));
    }
    Ok(NativeValue::Array(vec![item; n as usize]))
}

fn builtin_list_range(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let from = expect_int(&args[0])?;
    let to = expect_int(&args[1])?;
    let mut out = Vec::new();
    if to > from {
        out.reserve((to - from) as usize);
        for i in from..to {
            out.push(NativeValue::Int(i));
        }
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_range_inclusive(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let from = expect_int(&args[0])?;
    let to = expect_int(&args[1])?;
    let mut out = Vec::new();
    if to >= from {
        out.reserve((to - from + 1) as usize);
        for i in from..=to {
            out.push(NativeValue::Int(i));
        }
    }
    Ok(NativeValue::Array(out))
}

// ---------------------------------------------------------------------------
// Access
// ---------------------------------------------------------------------------

fn builtin_list_len(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_list(&args[0])?;
    Ok(NativeValue::Int(l.len() as i64))
}

fn builtin_list_is_empty(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_list(&args[0])?;
    Ok(NativeValue::Bool(l.is_empty()))
}

fn builtin_list_get(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?;
    let i = expect_int(&args[1])?;
    if i < 0 {
        return Ok(option_none());
    }
    match l.get(i as usize) {
        Some(v) => Ok(option_some(v.clone())),
        None => Ok(option_none()),
    }
}

fn builtin_list_first(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_list(&args[0])?;
    Ok(match l.first() {
        Some(v) => option_some(v.clone()),
        None => option_none(),
    })
}

fn builtin_list_last(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_list(&args[0])?;
    Ok(match l.last() {
        Some(v) => option_some(v.clone()),
        None => option_none(),
    })
}

fn builtin_list_slice(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let l = expect_list(&args[0])?;
    let from = expect_int(&args[1])?.max(0) as usize;
    let to = expect_int(&args[2])?.max(0) as usize;
    let len = l.len();
    let lo = from.min(len);
    let hi = to.min(len).max(lo);
    Ok(NativeValue::Array(l[lo..hi].to_vec()))
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

fn builtin_list_contains(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?;
    let needle = &args[1];
    Ok(NativeValue::Bool(l.iter().any(|v| values_equal(v, needle))))
}

fn builtin_list_find(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    for item in l.into_iter() {
        let r = invoke_closure(f, &[item.clone()])?;
        if is_truthy(&r)? {
            return Ok(option_some(item));
        }
    }
    Ok(option_none())
}

fn builtin_list_find_index(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    for (i, item) in l.into_iter().enumerate() {
        let r = invoke_closure(f, &[item])?;
        if is_truthy(&r)? {
            return Ok(option_some(NativeValue::Int(i as i64)));
        }
    }
    Ok(option_none())
}

fn builtin_list_index_of(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?;
    let needle = &args[1];
    for (i, v) in l.iter().enumerate() {
        if values_equal(v, needle) {
            return Ok(option_some(NativeValue::Int(i as i64)));
        }
    }
    Ok(option_none())
}

// ---------------------------------------------------------------------------
// Transform
// ---------------------------------------------------------------------------

fn builtin_list_map(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    let mut out = Vec::with_capacity(l.len());
    for item in l {
        out.push(invoke_closure(f, &[item])?);
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_flat_map(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    let mut out = Vec::new();
    for item in l {
        let r = invoke_closure(f, &[item])?;
        let inner = expect_list(&r)?.clone();
        out.extend(inner);
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_filter(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    let mut out = Vec::new();
    for item in l {
        let r = invoke_closure(f, &[item.clone()])?;
        if is_truthy(&r)? {
            out.push(item);
        }
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_filter_map(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    let mut out = Vec::new();
    for item in l {
        let r = invoke_closure(f, &[item])?;
        // Decode `Some(x)` (1-elt array) vs `None` (Unit).
        match r {
            NativeValue::Unit => continue,
            NativeValue::Array(mut inner) if inner.len() == 1 => {
                out.push(inner.pop().unwrap());
            }
            other => return Err(format!("list::filter_map: expected Option, got {:?}", other)),
        }
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_reduce(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    let mut iter = l.into_iter();
    let mut acc = match iter.next() {
        Some(v) => v,
        None => return Ok(option_none()),
    };
    for item in iter {
        acc = invoke_closure(f, &[acc, item])?;
    }
    Ok(option_some(acc))
}

fn builtin_list_fold(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let l = expect_list(&args[0])?.clone();
    let mut acc = args[1].clone();
    let f = &args[2];
    for item in l {
        acc = invoke_closure(f, &[acc, item])?;
    }
    Ok(acc)
}

fn builtin_list_scan(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let l = expect_list(&args[0])?.clone();
    let init = args[1].clone();
    let f = &args[2];
    let mut acc = init.clone();
    let mut out = Vec::with_capacity(l.len() + 1);
    out.push(acc.clone());
    for item in l {
        acc = invoke_closure(f, &[acc, item])?;
        out.push(acc.clone());
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_zip(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_list(&args[0])?;
    let b = expect_list(&args[1])?;
    let n = a.len().min(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(pair(a[i].clone(), b[i].clone()));
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_zip_with(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let a = expect_list(&args[0])?.clone();
    let b = expect_list(&args[1])?.clone();
    let f = &args[2];
    let n = a.len().min(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(invoke_closure(f, &[a[i].clone(), b[i].clone()])?);
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_enumerate(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_list(&args[0])?;
    let out: Vec<NativeValue> = l
        .iter()
        .enumerate()
        .map(|(i, v)| pair(NativeValue::Int(i as i64), v.clone()))
        .collect();
    Ok(NativeValue::Array(out))
}

fn builtin_list_flatten(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let outer = expect_list(&args[0])?;
    let mut out = Vec::new();
    for item in outer {
        let inner = expect_list(item)?;
        out.extend(inner.iter().cloned());
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_chunk(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?;
    let size = expect_int(&args[1])?;
    if size <= 0 {
        return Err(format!("list::chunk: invalid chunk size {}", size));
    }
    let size = size as usize;
    let out: Vec<NativeValue> = l
        .chunks(size)
        .map(|c| NativeValue::Array(c.to_vec()))
        .collect();
    Ok(NativeValue::Array(out))
}

fn builtin_list_window(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?;
    let size = expect_int(&args[1])?;
    if size <= 0 {
        return Err(format!("list::window: invalid window size {}", size));
    }
    let size = size as usize;
    let mut out = Vec::new();
    if l.len() >= size {
        for w in l.windows(size) {
            out.push(NativeValue::Array(w.to_vec()));
        }
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_take(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?;
    let n = expect_int(&args[1])?.max(0) as usize;
    Ok(NativeValue::Array(l.iter().take(n).cloned().collect()))
}

fn builtin_list_drop(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?;
    let n = expect_int(&args[1])?.max(0) as usize;
    Ok(NativeValue::Array(l.iter().skip(n).cloned().collect()))
}

fn builtin_list_take_while(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    let mut out = Vec::new();
    for item in l {
        let r = invoke_closure(f, &[item.clone()])?;
        if is_truthy(&r)? {
            out.push(item);
        } else {
            break;
        }
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_drop_while(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    let mut out = Vec::new();
    let mut dropping = true;
    for item in l {
        if dropping {
            let r = invoke_closure(f, &[item.clone()])?;
            if is_truthy(&r)? {
                continue;
            }
            dropping = false;
        }
        out.push(item);
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_partition(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    let mut truthy = Vec::new();
    let mut falsy = Vec::new();
    for item in l {
        let r = invoke_closure(f, &[item.clone()])?;
        if is_truthy(&r)? {
            truthy.push(item);
        } else {
            falsy.push(item);
        }
    }
    Ok(pair(NativeValue::Array(truthy), NativeValue::Array(falsy)))
}

fn builtin_list_unzip(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_list(&args[0])?;
    let mut left = Vec::with_capacity(l.len());
    let mut right = Vec::with_capacity(l.len());
    for item in l {
        let p = expect_list(item)?;
        if p.len() != 2 {
            return Err(format!("list::unzip: expected pair, got len {}", p.len()));
        }
        left.push(p[0].clone());
        right.push(p[1].clone());
    }
    Ok(pair(NativeValue::Array(left), NativeValue::Array(right)))
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

fn builtin_list_any(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    for item in l {
        let r = invoke_closure(f, &[item])?;
        if is_truthy(&r)? {
            return Ok(NativeValue::Bool(true));
        }
    }
    Ok(NativeValue::Bool(false))
}

fn builtin_list_all(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    for item in l {
        let r = invoke_closure(f, &[item])?;
        if !is_truthy(&r)? {
            return Ok(NativeValue::Bool(false));
        }
    }
    Ok(NativeValue::Bool(true))
}

fn builtin_list_count(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let f = &args[1];
    let mut n: i64 = 0;
    for item in l {
        let r = invoke_closure(f, &[item])?;
        if is_truthy(&r)? {
            n += 1;
        }
    }
    Ok(NativeValue::Int(n))
}

fn builtin_list_sum(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_list(&args[0])?;
    let mut total: i64 = 0;
    for item in l {
        match item {
            NativeValue::Int(n) => total = total.wrapping_add(*n),
            other => return Err(format!("list::sum: expected Int, got {:?}", other)),
        }
    }
    Ok(NativeValue::Int(total))
}

fn builtin_list_sum_float(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_list(&args[0])?;
    let mut total = 0.0_f64;
    for item in l {
        match item {
            NativeValue::Float(f) => total += *f,
            other => return Err(format!("list::sum_float: expected Float, got {:?}", other)),
        }
    }
    Ok(NativeValue::Float(total))
}

fn cmp_ord_supported(a: &NativeValue, b: &NativeValue) -> Result<std::cmp::Ordering, String> {
    use std::cmp::Ordering;
    match (a, b) {
        (NativeValue::Int(x), NativeValue::Int(y)) => Ok(x.cmp(y)),
        (NativeValue::Float(x), NativeValue::Float(y)) => {
            x.partial_cmp(y).ok_or_else(|| "list: NaN comparison".to_string())
        }
        (NativeValue::Str(x), NativeValue::Str(y)) => Ok(x.cmp(y)),
        _ => Err(format!(
            "list: min/max only support uniform Int / Float / Str (got {:?} vs {:?})",
            a, b
        )),
    }
    .map(|o| match o {
        Ordering::Less => Ordering::Less,
        Ordering::Equal => Ordering::Equal,
        Ordering::Greater => Ordering::Greater,
    })
}

fn builtin_list_min(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_list(&args[0])?;
    let mut iter = l.iter();
    let first = match iter.next() {
        Some(v) => v.clone(),
        None => return Ok(option_none()),
    };
    let mut best = first;
    for v in iter {
        if cmp_ord_supported(v, &best)? == std::cmp::Ordering::Less {
            best = v.clone();
        }
    }
    Ok(option_some(best))
}

fn builtin_list_max(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_list(&args[0])?;
    let mut iter = l.iter();
    let first = match iter.next() {
        Some(v) => v.clone(),
        None => return Ok(option_none()),
    };
    let mut best = first;
    for v in iter {
        if cmp_ord_supported(v, &best)? == std::cmp::Ordering::Greater {
            best = v.clone();
        }
    }
    Ok(option_some(best))
}

// ---------------------------------------------------------------------------
// Structural
// ---------------------------------------------------------------------------

fn builtin_list_concat(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_list(&args[0])?.clone();
    let b = expect_list(&args[1])?.clone();
    let mut out = Vec::with_capacity(a.len() + b.len());
    out.extend(a);
    out.extend(b);
    Ok(NativeValue::Array(out))
}

fn builtin_list_prepend(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let item = args[0].clone();
    let l = expect_list(&args[1])?.clone();
    let mut out = Vec::with_capacity(l.len() + 1);
    out.push(item);
    out.extend(l);
    Ok(NativeValue::Array(out))
}

fn builtin_list_append(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let mut out = expect_list(&args[0])?.clone();
    out.push(args[1].clone());
    Ok(NativeValue::Array(out))
}

fn builtin_list_insert(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let l = expect_list(&args[0])?.clone();
    let at = expect_int(&args[1])?;
    let item = args[2].clone();
    if at < 0 || at as usize > l.len() {
        return Err(format!(
            "list::insert: index {} out of bounds (len {})",
            at,
            l.len()
        ));
    }
    let mut out = l;
    out.insert(at as usize, item);
    Ok(NativeValue::Array(out))
}

fn builtin_list_remove(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?.clone();
    let at = expect_int(&args[1])?;
    if at < 0 || at as usize >= l.len() {
        return Err(format!(
            "list::remove: index {} out of bounds (len {})",
            at,
            l.len()
        ));
    }
    let mut out = l;
    out.remove(at as usize);
    Ok(NativeValue::Array(out))
}

fn builtin_list_reverse(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let mut out = expect_list(&args[0])?.clone();
    out.reverse();
    Ok(NativeValue::Array(out))
}

fn builtin_list_unique(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_list(&args[0])?;
    let mut out: Vec<NativeValue> = Vec::new();
    for item in l {
        if !out.iter().any(|seen| values_equal(seen, item)) {
            out.push(item.clone());
        }
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_intersperse(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?;
    let sep = args[1].clone();
    let mut out = Vec::with_capacity(l.len() * 2);
    for (i, v) in l.iter().enumerate() {
        if i > 0 {
            out.push(sep.clone());
        }
        out.push(v.clone());
    }
    Ok(NativeValue::Array(out))
}

fn builtin_list_join_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let l = expect_list(&args[0])?;
    let sep = expect_str(&args[1])?;
    let mut parts: Vec<String> = Vec::with_capacity(l.len());
    for v in l {
        parts.push(expect_str(v)?.clone());
    }
    Ok(NativeValue::Str(parts.join(sep.as_str())))
}

// ---------------------------------------------------------------------------
// Mutable builder
// ---------------------------------------------------------------------------
//
// Until the dedicated `ListMut` variant is added, we represent it as a
// thread-local handle table keyed by Int. Each `builder()` call returns a
// fresh Int handle; `push` / `pop` / `build` operate on that slot. This is
// only used inside this module's tests; integration replaces it with the
// real `ListMut(Rc<RefCell<...>>)` variant.

thread_local! {
    static LIST_BUILDERS: RefCell<Vec<Option<Rc<RefCell<Vec<NativeValue>>>>>> =
        const { RefCell::new(Vec::new()) };
}

fn alloc_builder() -> i64 {
    LIST_BUILDERS.with(|cell| {
        let mut v = cell.borrow_mut();
        let id = v.len() as i64;
        v.push(Some(Rc::new(RefCell::new(Vec::new()))));
        id
    })
}

fn with_builder<F, R>(id: i64, f: F) -> Result<R, String>
where
    F: FnOnce(&Rc<RefCell<Vec<NativeValue>>>) -> R,
{
    LIST_BUILDERS.with(|cell| {
        let v = cell.borrow();
        let slot = v
            .get(id as usize)
            .ok_or_else(|| format!("list: invalid builder handle {}", id))?;
        let b = slot
            .as_ref()
            .ok_or_else(|| format!("list: builder {} already consumed", id))?;
        Ok(f(b))
    })
}

fn take_builder(id: i64) -> Result<Vec<NativeValue>, String> {
    LIST_BUILDERS.with(|cell| {
        let mut v = cell.borrow_mut();
        let slot = v
            .get_mut(id as usize)
            .ok_or_else(|| format!("list: invalid builder handle {}", id))?;
        let b = slot
            .take()
            .ok_or_else(|| format!("list: builder {} already consumed", id))?;
        let inner = Rc::try_unwrap(b)
            .map_err(|_| "list: builder still has outstanding references".to_string())?
            .into_inner();
        Ok(inner)
    })
}

fn builtin_list_builder(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::Int(alloc_builder()))
}

fn builtin_list_push(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let id = expect_int(&args[0])?;
    let item = args[1].clone();
    with_builder(id, |b| b.borrow_mut().push(item))?;
    Ok(NativeValue::Unit)
}

fn builtin_list_pop(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let id = expect_int(&args[0])?;
    let popped = with_builder(id, |b| b.borrow_mut().pop())?;
    Ok(match popped {
        Some(v) => option_some(v),
        None => option_none(),
    })
}

fn builtin_list_build(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let id = expect_int(&args[0])?;
    let v = take_builder(id)?;
    Ok(NativeValue::Array(v))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    // Construction
    registry.register(interner.intern("list_new"),             builtin_list_new);
    registry.register(interner.intern("list_of"),              builtin_list_of);
    registry.register(interner.intern("list_repeat"),          builtin_list_repeat);
    registry.register(interner.intern("list_range"),           builtin_list_range);
    registry.register(interner.intern("list_range_inclusive"), builtin_list_range_inclusive);

    // Access
    registry.register(interner.intern("list_len"),       builtin_list_len);
    registry.register(interner.intern("list_is_empty"),  builtin_list_is_empty);
    registry.register(interner.intern("list_get"),       builtin_list_get);
    registry.register(interner.intern("list_first"),     builtin_list_first);
    registry.register(interner.intern("list_last"),      builtin_list_last);
    registry.register(interner.intern("list_slice"),     builtin_list_slice);

    // Search
    registry.register(interner.intern("list_contains"),   builtin_list_contains);
    registry.register(interner.intern("list_find"),       builtin_list_find);
    registry.register(interner.intern("list_find_index"), builtin_list_find_index);
    registry.register(interner.intern("list_index_of"),   builtin_list_index_of);

    // Transform
    registry.register(interner.intern("list_map"),         builtin_list_map);
    registry.register(interner.intern("list_flat_map"),    builtin_list_flat_map);
    registry.register(interner.intern("list_filter"),      builtin_list_filter);
    registry.register(interner.intern("list_filter_map"),  builtin_list_filter_map);
    registry.register(interner.intern("list_reduce"),      builtin_list_reduce);
    registry.register(interner.intern("list_fold"),        builtin_list_fold);
    registry.register(interner.intern("list_scan"),        builtin_list_scan);
    registry.register(interner.intern("list_zip"),         builtin_list_zip);
    registry.register(interner.intern("list_zip_with"),    builtin_list_zip_with);
    registry.register(interner.intern("list_enumerate"),   builtin_list_enumerate);
    registry.register(interner.intern("list_flatten"),     builtin_list_flatten);
    registry.register(interner.intern("list_chunk"),       builtin_list_chunk);
    registry.register(interner.intern("list_window"),      builtin_list_window);
    registry.register(interner.intern("list_take"),        builtin_list_take);
    registry.register(interner.intern("list_drop"),        builtin_list_drop);
    registry.register(interner.intern("list_take_while"),  builtin_list_take_while);
    registry.register(interner.intern("list_drop_while"),  builtin_list_drop_while);
    registry.register(interner.intern("list_partition"),   builtin_list_partition);
    registry.register(interner.intern("list_unzip"),       builtin_list_unzip);

    // Aggregation
    registry.register(interner.intern("list_any"),       builtin_list_any);
    registry.register(interner.intern("list_all"),       builtin_list_all);
    registry.register(interner.intern("list_count"),     builtin_list_count);
    registry.register(interner.intern("list_sum"),       builtin_list_sum);
    registry.register(interner.intern("list_sum_float"), builtin_list_sum_float);
    registry.register(interner.intern("list_min"),       builtin_list_min);
    registry.register(interner.intern("list_max"),       builtin_list_max);

    // Structural
    registry.register(interner.intern("list_concat"),      builtin_list_concat);
    registry.register(interner.intern("list_prepend"),     builtin_list_prepend);
    registry.register(interner.intern("list_append"),      builtin_list_append);
    registry.register(interner.intern("list_insert"),      builtin_list_insert);
    registry.register(interner.intern("list_remove"),      builtin_list_remove);
    registry.register(interner.intern("list_reverse"),     builtin_list_reverse);
    registry.register(interner.intern("list_unique"),      builtin_list_unique);
    registry.register(interner.intern("list_intersperse"), builtin_list_intersperse);
    registry.register(interner.intern("list_join_str"),    builtin_list_join_str);

    // Builder
    registry.register(interner.intern("list_builder"), builtin_list_builder);
    registry.register(interner.intern("list_push"),    builtin_list_push);
    registry.register(interner.intern("list_pop"),     builtin_list_pop);
    registry.register(interner.intern("list_build"),   builtin_list_build);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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

    // --- Construction ------------------------------------------------------

    #[test]
    fn new_yields_empty_list() {
        let r = builtin_list_new(&[]).unwrap();
        assert_eq!(r, NativeValue::Array(vec![]));
    }

    #[test]
    fn of_collects_args() {
        let r = builtin_list_of(&[NativeValue::Int(1), NativeValue::Int(2)]).unwrap();
        assert_eq!(r, ints(&[1, 2]));
    }

    #[test]
    fn repeat_produces_n_copies() {
        let r = builtin_list_repeat(&[NativeValue::Int(7), NativeValue::Int(3)]).unwrap();
        assert_eq!(r, ints(&[7, 7, 7]));
    }

    #[test]
    fn repeat_zero_is_empty() {
        let r = builtin_list_repeat(&[NativeValue::Int(7), NativeValue::Int(0)]).unwrap();
        assert_eq!(r, ints(&[]));
    }

    #[test]
    fn range_exclusive_end() {
        let r = builtin_list_range(&[NativeValue::Int(0), NativeValue::Int(5)]).unwrap();
        assert_eq!(r, ints(&[0, 1, 2, 3, 4]));
        assert_eq!(builtin_list_len(&[r]).unwrap(), NativeValue::Int(5));
    }

    #[test]
    fn range_inclusive_includes_end() {
        let r = builtin_list_range_inclusive(&[NativeValue::Int(0), NativeValue::Int(5)]).unwrap();
        assert_eq!(r, ints(&[0, 1, 2, 3, 4, 5]));
        assert_eq!(builtin_list_len(&[r]).unwrap(), NativeValue::Int(6));
    }

    #[test]
    fn range_empty_when_to_le_from() {
        let r = builtin_list_range(&[NativeValue::Int(5), NativeValue::Int(5)]).unwrap();
        assert_eq!(r, ints(&[]));
    }

    // --- Access ------------------------------------------------------------

    #[test]
    fn len_and_is_empty() {
        assert_eq!(
            builtin_list_len(&[ints(&[1, 2, 3])]).unwrap(),
            NativeValue::Int(3)
        );
        assert_eq!(
            builtin_list_is_empty(&[ints(&[])]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_list_is_empty(&[ints(&[1])]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn get_in_bounds_is_some() {
        let l = ints(&[10, 20, 30]);
        let r = builtin_list_get(&[l, NativeValue::Int(0)]).unwrap();
        assert_eq!(r, option_some(NativeValue::Int(10)));
    }

    #[test]
    fn get_out_of_bounds_is_none() {
        let l = ints(&[10, 20, 30]);
        let r = builtin_list_get(&[l, NativeValue::Int(99)]).unwrap();
        assert_eq!(r, option_none());
    }

    #[test]
    fn first_last_on_empty_is_none() {
        assert_eq!(builtin_list_first(&[ints(&[])]).unwrap(), option_none());
        assert_eq!(builtin_list_last(&[ints(&[])]).unwrap(), option_none());
    }

    #[test]
    fn first_last_on_nonempty() {
        let l = ints(&[1, 2, 3]);
        assert_eq!(
            builtin_list_first(&[l.clone()]).unwrap(),
            option_some(NativeValue::Int(1))
        );
        assert_eq!(
            builtin_list_last(&[l]).unwrap(),
            option_some(NativeValue::Int(3))
        );
    }

    #[test]
    fn slice_basic_and_clamped() {
        let l = ints(&[1, 2, 3, 4, 5]);
        assert_eq!(
            builtin_list_slice(&[l.clone(), NativeValue::Int(1), NativeValue::Int(4)]).unwrap(),
            ints(&[2, 3, 4])
        );
        // out-of-range clamp
        assert_eq!(
            builtin_list_slice(&[l, NativeValue::Int(3), NativeValue::Int(99)]).unwrap(),
            ints(&[4, 5])
        );
    }

    // --- Search ------------------------------------------------------------

    #[test]
    fn contains_uses_structural_eq() {
        let l = ints(&[1, 2, 3]);
        assert_eq!(
            builtin_list_contains(&[l.clone(), NativeValue::Int(2)]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_list_contains(&[l, NativeValue::Int(99)]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn index_of_returns_first_match() {
        let l = ints(&[10, 20, 30, 20]);
        assert_eq!(
            builtin_list_index_of(&[l.clone(), NativeValue::Int(20)]).unwrap(),
            option_some(NativeValue::Int(1))
        );
        assert_eq!(
            builtin_list_index_of(&[l, NativeValue::Int(99)]).unwrap(),
            option_none()
        );
    }

    #[test]
    #[ignore = "requires invoke_closure plumbing from integration step"]
    fn find_with_predicate() {
        // intent: find(is_even, [1,2,3,4]) -> Some(2)
    }

    #[test]
    #[ignore = "requires invoke_closure plumbing from integration step"]
    fn find_index_with_predicate() {
        // intent: find_index(is_even, [1,2,3,4]) -> Some(1)
    }

    // --- Transform ---------------------------------------------------------

    #[test]
    #[ignore = "requires invoke_closure: map(double, [1,2,3]) == [2,4,6]"]
    fn map_doubles_each_element() {}

    #[test]
    #[ignore = "requires invoke_closure: filter(is_even, [1,2,3,4]) == [2,4]"]
    fn filter_keeps_truthy() {}

    #[test]
    #[ignore = "requires invoke_closure: fold(add, 0, [1,2,3]) == 6"]
    fn fold_sums() {}

    #[test]
    #[ignore = "requires invoke_closure: reduce(add, [1,2,3]) == Some(6); reduce(add, []) == None"]
    fn reduce_handles_empty_and_nonempty() {}

    #[test]
    #[ignore = "requires invoke_closure: flat_map(|n| [n, n], [1,2]) == [1,1,2,2]"]
    fn flat_map_concatenates_results() {}

    #[test]
    #[ignore = "requires invoke_closure: filter_map(parse_int, ...) drops Nones"]
    fn filter_map_drops_nones() {}

    #[test]
    #[ignore = "requires invoke_closure: scan(add, 0, [1,2,3]) == [0,1,3,6]"]
    fn scan_collects_intermediate_states() {}

    #[test]
    fn zip_pairs_min_length() {
        let r = builtin_list_zip(&[ints(&[1, 2, 3]), strs(&["a", "b"])]).unwrap();
        let l = expect_list(&r).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(
            l[0],
            pair(NativeValue::Int(1), NativeValue::Str("a".into()))
        );
        assert_eq!(
            l[1],
            pair(NativeValue::Int(2), NativeValue::Str("b".into()))
        );
    }

    #[test]
    fn enumerate_pairs_index_with_value() {
        let r = builtin_list_enumerate(&[strs(&["a", "b"])]).unwrap();
        let l = expect_list(&r).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(
            l[0],
            pair(NativeValue::Int(0), NativeValue::Str("a".into()))
        );
        assert_eq!(
            l[1],
            pair(NativeValue::Int(1), NativeValue::Str("b".into()))
        );
    }

    #[test]
    fn flatten_concatenates_lists() {
        let nested =
            NativeValue::Array(vec![ints(&[1, 2]), ints(&[3]), ints(&[]), ints(&[4, 5])]);
        let r = builtin_list_flatten(&[nested]).unwrap();
        assert_eq!(r, ints(&[1, 2, 3, 4, 5]));
    }

    #[test]
    fn chunk_partitions_with_remainder() {
        let r =
            builtin_list_chunk(&[ints(&[1, 2, 3, 4, 5]), NativeValue::Int(2)]).unwrap();
        assert_eq!(
            r,
            NativeValue::Array(vec![ints(&[1, 2]), ints(&[3, 4]), ints(&[5])])
        );
    }

    #[test]
    fn chunk_zero_is_error() {
        let r = builtin_list_chunk(&[ints(&[1, 2, 3]), NativeValue::Int(0)]);
        assert!(r.is_err());
    }

    #[test]
    fn window_slides_by_one() {
        let r =
            builtin_list_window(&[ints(&[1, 2, 3, 4]), NativeValue::Int(2)]).unwrap();
        assert_eq!(
            r,
            NativeValue::Array(vec![ints(&[1, 2]), ints(&[2, 3]), ints(&[3, 4])])
        );
    }

    #[test]
    fn window_zero_is_error() {
        let r = builtin_list_window(&[ints(&[1, 2, 3]), NativeValue::Int(0)]);
        assert!(r.is_err());
    }

    #[test]
    fn take_and_drop() {
        assert_eq!(
            builtin_list_take(&[ints(&[1, 2, 3, 4]), NativeValue::Int(2)]).unwrap(),
            ints(&[1, 2])
        );
        assert_eq!(
            builtin_list_drop(&[ints(&[1, 2, 3, 4]), NativeValue::Int(2)]).unwrap(),
            ints(&[3, 4])
        );
        // n > len clamps
        assert_eq!(
            builtin_list_take(&[ints(&[1, 2]), NativeValue::Int(99)]).unwrap(),
            ints(&[1, 2])
        );
        assert_eq!(
            builtin_list_drop(&[ints(&[1, 2]), NativeValue::Int(99)]).unwrap(),
            ints(&[])
        );
    }

    #[test]
    #[ignore = "requires invoke_closure: take_while/drop_while predicate behaviour"]
    fn take_while_and_drop_while() {}

    #[test]
    #[ignore = "requires invoke_closure: partition(is_even, [1,2,3,4]) == ([2,4],[1,3])"]
    fn partition_splits_truthy_from_falsy() {}

    #[test]
    fn unzip_inverts_zip() {
        let zipped = builtin_list_zip(&[ints(&[1, 2, 3]), strs(&["a", "b", "c"])]).unwrap();
        let r = builtin_list_unzip(&[zipped]).unwrap();
        assert_eq!(r, pair(ints(&[1, 2, 3]), strs(&["a", "b", "c"])));
    }

    // --- Aggregation -------------------------------------------------------

    #[test]
    #[ignore = "requires invoke_closure: any/all/count predicate behaviour"]
    fn any_all_count() {}

    #[test]
    fn sum_int() {
        assert_eq!(
            builtin_list_sum(&[ints(&[1, 2, 3])]).unwrap(),
            NativeValue::Int(6)
        );
        assert_eq!(builtin_list_sum(&[ints(&[])]).unwrap(), NativeValue::Int(0));
    }

    #[test]
    fn sum_int_rejects_mixed() {
        let mixed = NativeValue::Array(vec![NativeValue::Int(1), NativeValue::Float(2.0)]);
        assert!(builtin_list_sum(&[mixed]).is_err());
    }

    #[test]
    fn sum_float_basic() {
        let l = NativeValue::Array(vec![NativeValue::Float(1.0), NativeValue::Float(2.5)]);
        assert_eq!(
            builtin_list_sum_float(&[l]).unwrap(),
            NativeValue::Float(3.5)
        );
    }

    #[test]
    fn min_max_int() {
        let l = ints(&[3, 1, 4, 1, 5, 9, 2, 6]);
        assert_eq!(
            builtin_list_min(&[l.clone()]).unwrap(),
            option_some(NativeValue::Int(1))
        );
        assert_eq!(
            builtin_list_max(&[l]).unwrap(),
            option_some(NativeValue::Int(9))
        );
    }

    #[test]
    fn min_max_str() {
        let l = strs(&["b", "a", "c"]);
        assert_eq!(
            builtin_list_min(&[l.clone()]).unwrap(),
            option_some(NativeValue::Str("a".into()))
        );
        assert_eq!(
            builtin_list_max(&[l]).unwrap(),
            option_some(NativeValue::Str("c".into()))
        );
    }

    #[test]
    fn min_max_float() {
        let l = NativeValue::Array(vec![
            NativeValue::Float(1.5),
            NativeValue::Float(0.5),
            NativeValue::Float(2.5),
        ]);
        assert_eq!(
            builtin_list_min(&[l.clone()]).unwrap(),
            option_some(NativeValue::Float(0.5))
        );
        assert_eq!(
            builtin_list_max(&[l]).unwrap(),
            option_some(NativeValue::Float(2.5))
        );
    }

    #[test]
    fn min_max_empty_is_none() {
        assert_eq!(builtin_list_min(&[ints(&[])]).unwrap(), option_none());
        assert_eq!(builtin_list_max(&[ints(&[])]).unwrap(), option_none());
    }

    #[test]
    fn min_mixed_types_is_error() {
        let mixed = NativeValue::Array(vec![NativeValue::Int(1), NativeValue::Str("x".into())]);
        assert!(builtin_list_min(&[mixed]).is_err());
    }

    // --- Structural --------------------------------------------------------

    #[test]
    fn concat_appends() {
        let r = builtin_list_concat(&[ints(&[1, 2]), ints(&[3, 4])]).unwrap();
        assert_eq!(r, ints(&[1, 2, 3, 4]));
    }

    #[test]
    fn prepend_and_append() {
        assert_eq!(
            builtin_list_prepend(&[NativeValue::Int(0), ints(&[1, 2])]).unwrap(),
            ints(&[0, 1, 2])
        );
        assert_eq!(
            builtin_list_append(&[ints(&[1, 2]), NativeValue::Int(3)]).unwrap(),
            ints(&[1, 2, 3])
        );
    }

    #[test]
    fn insert_in_middle() {
        let r = builtin_list_insert(&[
            ints(&[1, 3]),
            NativeValue::Int(1),
            NativeValue::Int(2),
        ])
        .unwrap();
        assert_eq!(r, ints(&[1, 2, 3]));
    }

    #[test]
    fn insert_out_of_bounds_is_error() {
        let r = builtin_list_insert(&[
            ints(&[1, 2]),
            NativeValue::Int(99),
            NativeValue::Int(5),
        ]);
        assert!(r.is_err());
    }

    #[test]
    fn remove_in_middle() {
        let r = builtin_list_remove(&[ints(&[1, 2, 3]), NativeValue::Int(1)]).unwrap();
        assert_eq!(r, ints(&[1, 3]));
    }

    #[test]
    fn remove_out_of_bounds_is_error() {
        let r = builtin_list_remove(&[ints(&[1]), NativeValue::Int(7)]);
        assert!(r.is_err());
    }

    #[test]
    fn reverse_reverses() {
        assert_eq!(
            builtin_list_reverse(&[ints(&[1, 2, 3])]).unwrap(),
            ints(&[3, 2, 1])
        );
    }

    #[test]
    fn unique_preserves_first_occurrence() {
        let l = ints(&[1, 2, 1, 3, 2, 4]);
        assert_eq!(
            builtin_list_unique(&[l]).unwrap(),
            ints(&[1, 2, 3, 4])
        );
    }

    #[test]
    fn intersperse_inserts_between() {
        let r = builtin_list_intersperse(&[ints(&[1, 2, 3]), NativeValue::Int(0)]).unwrap();
        assert_eq!(r, ints(&[1, 0, 2, 0, 3]));
        // intersperse on empty / single is unchanged
        let r = builtin_list_intersperse(&[ints(&[]), NativeValue::Int(0)]).unwrap();
        assert_eq!(r, ints(&[]));
        let r = builtin_list_intersperse(&[ints(&[7]), NativeValue::Int(0)]).unwrap();
        assert_eq!(r, ints(&[7]));
    }

    #[test]
    fn join_str_concatenates_with_sep() {
        let r = builtin_list_join_str(&[
            strs(&["a", "b", "c"]),
            NativeValue::Str(",".into()),
        ])
        .unwrap();
        assert_eq!(r, NativeValue::Str("a,b,c".into()));
    }

    // --- Builder -----------------------------------------------------------

    #[test]
    fn builder_push_build() {
        let h = builtin_list_builder(&[]).unwrap();
        for i in 1..=3 {
            builtin_list_push(&[h.clone(), NativeValue::Int(i)]).unwrap();
        }
        let r = builtin_list_build(&[h]).unwrap();
        assert_eq!(r, ints(&[1, 2, 3]));
    }

    #[test]
    fn builder_pop_returns_some_then_none() {
        let h = builtin_list_builder(&[]).unwrap();
        builtin_list_push(&[h.clone(), NativeValue::Int(7)]).unwrap();
        let popped = builtin_list_pop(&[h.clone()]).unwrap();
        assert_eq!(popped, option_some(NativeValue::Int(7)));
        let empty_pop = builtin_list_pop(&[h.clone()]).unwrap();
        assert_eq!(empty_pop, option_none());
        let r = builtin_list_build(&[h]).unwrap();
        assert_eq!(r, ints(&[]));
    }

    // --- Registration ------------------------------------------------------

    #[test]
    fn register_populates_all_names() {
        let mut registry = NativeRegistry::new();
        let mut interner = Interner::new();
        register(&mut registry, &mut interner);

        for name in [
            "list_new", "list_of", "list_repeat", "list_range", "list_range_inclusive",
            "list_len", "list_is_empty", "list_get", "list_first", "list_last", "list_slice",
            "list_contains", "list_find", "list_find_index", "list_index_of",
            "list_map", "list_flat_map", "list_filter", "list_filter_map", "list_reduce",
            "list_fold", "list_scan", "list_zip", "list_zip_with", "list_enumerate",
            "list_flatten", "list_chunk", "list_window", "list_take", "list_drop",
            "list_take_while", "list_drop_while", "list_partition", "list_unzip",
            "list_any", "list_all", "list_count", "list_sum", "list_sum_float",
            "list_min", "list_max",
            "list_concat", "list_prepend", "list_append", "list_insert", "list_remove",
            "list_reverse", "list_unique", "list_intersperse", "list_join_str",
            "list_builder", "list_push", "list_pop", "list_build",
        ] {
            let sym = interner.intern(name);
            assert!(
                registry.get(sym).is_some(),
                "expected list register to include `{}`",
                name
            );
        }
    }
}
