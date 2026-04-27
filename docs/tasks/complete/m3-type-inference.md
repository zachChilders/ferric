# Task: M3 Type Inference Replacement

## Objective
Replace `ferric_typecheck` with `ferric_infer`, implementing full Hindley-Milner type inference with Algorithm J. Remove the `Ty::Unknown` escape hatch - every expression must have a concrete type.

## Architecture Context
- This is a **complete stage replacement**
- The public signature remains identical
- `main.rs` changes: one import swap
- All other stages remain completely untouched
- This proves the architecture's replacement mechanism works

## Public Interface (UNCHANGED)

```rust
// ferric_infer/src/lib.rs
// Signature is identical to ferric_typecheck
pub fn typecheck(ast: &ParseResult, resolve: &ResolveResult) -> TypeResult;
```

The function name stays `typecheck` even though the crate is `ferric_infer`.
This maintains the stage contract.

## Feature Requirements

### Remove Ty::Unknown
Delete the `Unknown` variant from `ferric_common::Ty`.
Every expression must resolve to a concrete type or produce a `TypeError::CannotInfer`.

### Type Inference with Algorithm J

Implement Hindley-Milner type inference:

1. **Type variables:** Fresh type variables for unknown types
2. **Constraint generation:** Walk AST, generate type equality constraints
3. **Unification:** Solve constraints to find most general types
4. **Occurs check:** Prevent infinite types
5. **Generalization:** ∀-quantification for let-bound polymorphism

### Type Scheme
Add to `ferric_common`:
```rust
pub struct TypeScheme {
    pub forall: Vec<TyVar>,  // quantified variables
    pub ty: Ty,
}

pub struct TyVar(pub u32);

// Extend Ty enum
pub enum Ty {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    Fn { params: Vec<Ty>, ret: Box<Ty> },
    Var(TyVar),  // type variable for inference
    // Unknown is DELETED
}
```

### Generic Functions
Support generic function definitions:
```rust
fn identity<T>(x: T) -> T {
    x
}

let a = identity(5)      // a: Int
let b = identity("hi")   // b: Str
```

The type scheme for `identity` is `∀T. T → T`.

## Algorithm J Implementation

### Type Inference Structure
```rust
struct TypeInfer<'a> {
    ast: &'a ParseResult,
    resolve: &'a ResolveResult,

    // State
    next_tyvar: u32,
    substitution: Substitution,
    env: TypeEnv,

    // Output
    node_types: HashMap<NodeId, Ty>,
    errors: Vec<TypeError>,
}

struct TypeEnv {
    schemes: HashMap<DefId, TypeScheme>,
}

struct Substitution {
    map: HashMap<TyVar, Ty>,
}

impl Substitution {
    fn apply(&self, ty: &Ty) -> Ty { ... }
    fn extend(&mut self, var: TyVar, ty: Ty) -> Result<(), TypeError> { ... }
    fn compose(&mut self, other: Substitution) { ... }
}
```

### Fresh Type Variables
```rust
impl TypeInfer {
    fn fresh_tyvar(&mut self) -> Ty {
        let var = TyVar(self.next_tyvar);
        self.next_tyvar += 1;
        Ty::Var(var)
    }
}
```

### Unification with Occurs Check
```rust
fn unify(&mut self, t1: &Ty, t2: &Ty, span: Span) -> Result<(), TypeError> {
    let t1 = self.substitution.apply(t1);
    let t2 = self.substitution.apply(t2);

    match (&t1, &t2) {
        (Ty::Int, Ty::Int) => Ok(()),
        (Ty::Float, Ty::Float) => Ok(()),
        (Ty::Bool, Ty::Bool) => Ok(()),
        (Ty::Str, Ty::Str) => Ok(()),
        (Ty::Unit, Ty::Unit) => Ok(()),

        (Ty::Var(v1), Ty::Var(v2)) if v1 == v2 => Ok(()),

        (Ty::Var(v), t) | (t, Ty::Var(v)) => {
            if occurs(*v, t) {
                Err(TypeError::InfiniteType { var: *v, ty: t.clone(), span })
            } else {
                self.substitution.extend(*v, t.clone())?;
                Ok(())
            }
        }

        (Ty::Fn { params: p1, ret: r1 }, Ty::Fn { params: p2, ret: r2 }) => {
            if p1.len() != p2.len() {
                return Err(TypeError::Mismatch { ... });
            }
            for (t1, t2) in p1.iter().zip(p2.iter()) {
                self.unify(t1, t2, span)?;
            }
            self.unify(r1, r2, span)
        }

        _ => Err(TypeError::Mismatch {
            expected: t1.clone(),
            found: t2.clone(),
            span,
        }),
    }
}

fn occurs(var: TyVar, ty: &Ty) -> bool {
    match ty {
        Ty::Var(v) => v == &var,
        Ty::Fn { params, ret } => {
            params.iter().any(|t| occurs(var, t)) || occurs(var, ret)
        }
        _ => false,
    }
}
```

### Type Inference Algorithm
```rust
fn infer_expr(&mut self, expr: &Expr) -> Result<Ty, TypeError> {
    let ty = match expr {
        Expr::Literal { value, .. } => {
            match value {
                Literal::Int(_) => Ty::Int,
                Literal::Float(_) => Ty::Float,
                Literal::Bool(_) => Ty::Bool,
                Literal::Str(_) => Ty::Str,
                Literal::Unit => Ty::Unit,
            }
        }

        Expr::Variable { id, span, .. } => {
            let def_id = self.resolve.resolutions[id];
            // Instantiate the type scheme
            self.instantiate(&self.env.schemes[&def_id])
        }

        Expr::Binary { op, left, right, span, .. } => {
            let t1 = self.infer_expr(left)?;
            let t2 = self.infer_expr(right)?;
            self.infer_binary_op(op, t1, t2, *span)?
        }

        Expr::Call { callee, args, span, .. } => {
            let fn_ty = self.infer_expr(callee)?;
            let arg_tys: Vec<_> = args.iter()
                .map(|arg| self.infer_expr(arg))
                .collect::<Result<_, _>>()?;

            let ret_ty = self.fresh_tyvar();

            let expected_fn_ty = Ty::Fn {
                params: arg_tys,
                ret: Box::new(ret_ty.clone()),
            };

            self.unify(&fn_ty, &expected_fn_ty, *span)?;
            ret_ty
        }

        Expr::If { cond, then_branch, else_branch, span, .. } => {
            let cond_ty = self.infer_expr(cond)?;
            self.unify(&cond_ty, &Ty::Bool, *span)?;

            let then_ty = self.infer_expr(then_branch)?;
            if let Some(else_br) = else_branch {
                let else_ty = self.infer_expr(else_br)?;
                self.unify(&then_ty, &else_ty, *span)?;
                then_ty
            } else {
                self.unify(&then_ty, &Ty::Unit, *span)?;
                Ty::Unit
            }
        }

        // ... other cases
    };

    // Apply current substitution to get concrete type
    let concrete_ty = self.substitution.apply(&ty);

    // If still has type variables, can't infer
    if has_type_vars(&concrete_ty) {
        return Err(TypeError::CannotInfer {
            expr: expr.clone(),
            span: expr.span(),
        });
    }

    self.node_types.insert(expr.id(), concrete_ty.clone());
    Ok(concrete_ty)
}
```

### Generalization (Let-Polymorphism)
```rust
fn generalize(&self, ty: &Ty) -> TypeScheme {
    let ty = self.substitution.apply(ty);
    let free_vars = free_type_vars(&ty);
    TypeScheme {
        forall: free_vars,
        ty,
    }
}

fn instantiate(&mut self, scheme: &TypeScheme) -> Ty {
    let mut subst = HashMap::new();
    for var in &scheme.forall {
        subst.insert(*var, self.fresh_tyvar());
    }
    apply_substitution(&scheme.ty, &subst)
}
```

## New Error Types

Add to `ferric_common::TypeError`:
```rust
pub enum TypeError {
    Mismatch { expected: Ty, found: Ty, span: Span },
    WrongArgumentCount { expected: usize, found: usize, span: Span },
    NotCallable { ty: Ty, span: Span },
    InfiniteType { var: TyVar, ty: Ty, span: Span },  // NEW
    CannotInfer { expr: Expr, span: Span },           // NEW
}
```

## Test Cases

Create unit tests for:
1. Simple inference: `let x = 5` infers `x: Int`
2. Function application: `fn f(x: Int) { x }; f(5)` type-checks
3. Generic identity: `fn id<T>(x: T) -> T { x }; id(5)` and `id("hi")` both work
4. If branches must match: `if true { 1 } else { "x" }` produces mismatch error
5. Occurs check: `fn f(x) { f }` produces infinite type error
6. Binary ops: `1 + 2` infers Int, `"a" + "b"` infers Str
7. Cannot infer ambiguous: proper error message
8. Let-polymorphism: `let id = |x| x; id(1); id("a")` uses id at two types
9. All M1 and M2 programs still type-check correctly
10. Programs that used `Ty::Unknown` now either infer correctly or produce errors

## Integration Changes

### In main.rs
```rust
// Change this line:
// use ferric_typecheck;
// To this:
use ferric_infer as ferric_typecheck;

// The rest of main.rs is UNCHANGED
```

That's it. One line. No other changes anywhere.

### In Cargo.toml
```toml
[dependencies]
# ferric_typecheck = { path = "crates/ferric_typecheck" }  # comment out
ferric_infer = { path = "crates/ferric_infer" }             # add this
```

## Acceptance Criteria

- [ ] `ferric_infer` crate created with identical public signature
- [ ] `Ty::Unknown` removed from `ferric_common`
- [ ] Algorithm J implemented with unification and occurs check
- [ ] Generic functions work (∀-quantification)
- [ ] Let-polymorphism works
- [ ] Type variables are resolved to concrete types
- [ ] Ambiguous types produce `CannotInfer` error
- [ ] Infinite types produce `InfiniteType` error
- [ ] All M1 and M2 tests still pass
- [ ] Integration required **only one line change in main.rs**
- [ ] All other stages completely untouched

## Critical Architecture Validation

**This milestone proves Rules 1 and 2 pay off:**
- Complete stage replacement
- Identical public signature
- **Only main.rs changes (one import swap)**
- Lexer, parser, resolver, VM, diagnostics, stdlib all untouched
- This is only possible because stages communicate through output types only

Document this success in the replacement log.

## Notes for Agent
- This is complex - take time to get unification right
- Test unification in isolation before full integration
- Occurs check is critical - test infinite type cases
- Make sure all type variables are resolved before returning
- The goal is to eliminate Unknown while maintaining the stage boundary
- All M1/M2 programs must still work - this is backward compatibility testing
