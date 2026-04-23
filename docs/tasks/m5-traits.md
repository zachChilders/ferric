# Task: M5 Traits and Generics

## Objective
Add user-defined traits, trait implementations, and generic functions with trait bounds. Replace the type checker again to add trait constraint solving on top of HM inference.

## Architecture Context
- This is a **type checker replacement** (third time)
- New stage: `ferric_traits` for collecting trait definitions and impls
- The public signature extends slightly to accept `TraitRegistry`
- `main.rs` changes: one import swap + one new stage call + one new argument
- All other stages remain untouched

## M5 Target Program

```rust
trait Describable {
    fn describe(self) -> Str
}

impl Describable for Int {
    fn describe(self) -> Str { "I am an integer: " + int_to_str(self) }
}

fn print_description<T: Describable>(val: T) {
    println(val.describe())
}

print_description(42)
```

## Changes to ferric_common

### Add to Item enum
```rust
pub enum Item {
    FnDef { ... },
    StructDef { ... },
    EnumDef { ... },

    TraitDef {      // NEW
        id: NodeId,
        name: Symbol,
        methods: Vec<TraitMethod>,
        span: Span,
    },

    ImplBlock {     // NEW
        id: NodeId,
        trait_name: Symbol,
        type_name: Symbol,
        methods: Vec<FnDef>,
        span: Span,
    },
}

pub struct TraitMethod {
    pub name: Symbol,
    pub params: Vec<(Symbol, TypeAnnotation)>,
    pub ret_ty: TypeAnnotation,
    pub span: Span,
}
```

### Add TraitRegistry
```rust
pub struct TraitRegistry {
    pub traits: HashMap<Symbol, TraitDef>,
    pub impls: HashMap<(Symbol, Ty), Vec<ImplDef>>,  // (trait_name, type) -> impls
}

pub struct TraitDef {
    pub name: Symbol,
    pub methods: HashMap<Symbol, MethodSignature>,
}

pub struct MethodSignature {
    pub params: Vec<Ty>,
    pub ret: Ty,
}

pub struct ImplDef {
    pub trait_name: Symbol,
    pub for_type: Ty,
    pub methods: HashMap<Symbol, DefId>,  // method name -> function DefId
}
```

### Extend TypeAnnotation for generics
```rust
pub enum TypeAnnotation {
    Named(Symbol),
    Generic {                  // NEW: T, U, etc.
        name: Symbol,
        bounds: Vec<Symbol>,   // trait bounds: T: Describable + Show
    },
    // ... existing variants
}
```

### Extend FnDef for generic functions
```rust
pub struct FnDef {
    pub id: NodeId,
    pub name: Symbol,
    pub type_params: Vec<TypeParam>,  // NEW: <T, U>
    pub params: Vec<(Symbol, TypeAnnotation)>,
    pub ret_ty: TypeAnnotation,
    pub body: Expr,
    pub span: Span,
}

pub struct TypeParam {
    pub name: Symbol,
    pub bounds: Vec<Symbol>,  // trait bounds
}
```

### Add to Expr for method calls
```rust
pub enum Expr {
    // ... existing variants

    MethodCall {    // NEW: val.describe()
        receiver: Box<Expr>,
        method: Symbol,
        args: Vec<Expr>,
        id: NodeId,
        span: Span,
    },
}
```

## Stage Changes

### 1. ferric_lexer Additions

Add keywords:
```rust
"trait" => TokenKind::Trait,
"impl" => TokenKind::Impl,
"for" => TokenKind::For,
```

### 2. ferric_parser Additions

**Parse trait definition:**
```rust
fn parse_trait_def(&mut self) -> Item {
    self.expect(TokenKind::Trait, "expected 'trait'");
    let name = self.expect_ident();
    self.expect(TokenKind::LBrace, "expected '{'");

    let mut methods = vec![];
    while !self.check(TokenKind::RBrace) {
        self.expect(TokenKind::Fn, "expected 'fn'");
        let method_name = self.expect_ident();
        self.expect(TokenKind::LParen, "expected '('");

        // Parse self parameter
        self.expect_ident("self");

        // Parse other parameters
        let mut params = vec![];
        if self.check(TokenKind::Comma) {
            self.advance();
            params = self.parse_params();
        }

        self.expect(TokenKind::RParen, "expected ')'");

        let ret_ty = if self.check(TokenKind::Arrow) {
            self.advance();
            self.parse_type()
        } else {
            TypeAnnotation::Named(self.interner.intern("Unit"))
        };

        methods.push(TraitMethod {
            name: method_name,
            params,
            ret_ty,
            span: ...
        });

        if !self.check(TokenKind::RBrace) {
            self.expect(TokenKind::Semi, "expected ';'");
        }
    }

    self.expect(TokenKind::RBrace, "expected '}'");
    Item::TraitDef { name, methods, ... }
}
```

**Parse impl block:**
```rust
fn parse_impl_block(&mut self) -> Item {
    self.expect(TokenKind::Impl, "expected 'impl'");
    let trait_name = self.expect_ident();
    self.expect(TokenKind::For, "expected 'for'");
    let type_name = self.expect_ident();
    self.expect(TokenKind::LBrace, "expected '{'");

    let mut methods = vec![];
    while !self.check(TokenKind::RBrace) {
        methods.push(self.parse_fn_def());
    }

    self.expect(TokenKind::RBrace, "expected '}'");
    Item::ImplBlock { trait_name, type_name, methods, ... }
}
```

**Parse generic function:**
```rust
fn parse_fn_def(&mut self) -> Item {
    self.expect(TokenKind::Fn, "expected 'fn'");
    let name = self.expect_ident();

    // Parse type parameters: <T, U: Bound>
    let type_params = if self.check(TokenKind::Lt) {
        self.parse_type_params()
    } else {
        vec![]
    };

    // ... rest of function parsing
}

fn parse_type_params(&mut self) -> Vec<TypeParam> {
    self.expect(TokenKind::Lt, "expected '<'");
    let mut params = vec![];

    while !self.check(TokenKind::Gt) {
        let name = self.expect_ident();
        let bounds = if self.check(TokenKind::Colon) {
            self.advance();
            self.parse_trait_bounds()
        } else {
            vec![]
        };

        params.push(TypeParam { name, bounds });

        if !self.check(TokenKind::Gt) {
            self.expect(TokenKind::Comma, "expected ','");
        }
    }

    self.expect(TokenKind::Gt, "expected '>'");
    params
}
```

**Parse method call:**
```rust
fn parse_postfix_expr(&mut self) -> Expr {
    let mut expr = self.parse_primary();

    loop {
        if self.check(TokenKind::Dot) {
            self.advance();
            let method = self.expect_ident();

            if self.check(TokenKind::LParen) {
                // Method call
                self.advance();
                let args = self.parse_args();
                self.expect(TokenKind::RParen, "expected ')'");
                expr = Expr::MethodCall {
                    receiver: Box::new(expr),
                    method,
                    args,
                    id: self.node_id_gen.next(),
                    span: ...
                };
            } else {
                // Field access
                expr = Expr::FieldAccess { ... };
            }
        }
        // ... other postfix operators
    }

    expr
}
```

### 3. NEW STAGE: ferric_traits

**Create trait registry builder:**

```rust
// ferric_traits/src/lib.rs
pub fn build_registry(ast: &ParseResult, interner: &Interner) -> TraitRegistry;

pub fn build_registry(ast: &ParseResult, interner: &Interner) -> TraitRegistry {
    let mut registry = TraitRegistry {
        traits: HashMap::new(),
        impls: HashMap::new(),
    };

    // Collect trait definitions
    for item in &ast.items {
        if let Item::TraitDef { name, methods, .. } = item {
            let mut method_sigs = HashMap::new();
            for method in methods {
                let sig = MethodSignature {
                    params: method.params.iter().map(|(_, ty)| convert_type(ty)).collect(),
                    ret: convert_type(&method.ret_ty),
                };
                method_sigs.insert(method.name, sig);
            }

            registry.traits.insert(*name, TraitDef {
                name: *name,
                methods: method_sigs,
            });
        }
    }

    // Collect impl blocks
    for item in &ast.items {
        if let Item::ImplBlock { trait_name, type_name, methods, .. } = item {
            let for_type = Ty::Named(*type_name);  // simplified

            let mut method_map = HashMap::new();
            for method in methods {
                // Methods get DefIds during resolution
                // For now, just record that this impl exists
            }

            let impl_def = ImplDef {
                trait_name: *trait_name,
                for_type: for_type.clone(),
                methods: method_map,
            };

            registry.impls.entry((*trait_name, for_type))
                .or_insert_with(Vec::new)
                .push(impl_def);
        }
    }

    // Register built-in trait impls
    register_builtin_impls(&mut registry, interner);

    registry
}

fn register_builtin_impls(registry: &mut TraitRegistry, interner: &Interner) {
    // Example: Display trait for Int, Float, Str, Bool
    let display_trait = interner.intern("Display");

    for ty in &[Ty::Int, Ty::Float, Ty::Str, Ty::Bool] {
        registry.impls.insert((display_trait, ty.clone()), vec![
            ImplDef {
                trait_name: display_trait,
                for_type: ty.clone(),
                methods: HashMap::new(),  // Built-in methods handled specially
            }
        ]);
    }
}
```

### 4. ferric_infer REPLACEMENT (Third Time)

**Extend the signature:**
```rust
// Was:
pub fn typecheck(ast: &ParseResult, resolve: &ResolveResult) -> TypeResult;

// Now:
pub fn typecheck(
    ast: &ParseResult,
    resolve: &ResolveResult,
    registry: &TraitRegistry,  // NEW argument
) -> TypeResult;
```

**Add constraint solving:**
```rust
struct TypeInfer<'a> {
    // ... existing fields
    registry: &'a TraitRegistry,  // NEW
    constraints: Vec<TraitConstraint>,  // NEW
}

struct TraitConstraint {
    ty: Ty,
    trait_name: Symbol,
    span: Span,
}

impl<'a> TypeInfer<'a> {
    fn add_constraint(&mut self, ty: Ty, trait_name: Symbol, span: Span) {
        self.constraints.push(TraitConstraint { ty, trait_name, span });
    }

    fn check_constraints(&mut self) -> Result<(), TypeError> {
        for constraint in &self.constraints {
            let ty = self.substitution.apply(&constraint.ty);

            // Check if there's an impl for this type and trait
            if !self.has_impl(&ty, constraint.trait_name) {
                return Err(TypeError::TraitNotImplemented {
                    ty: ty.clone(),
                    trait_name: constraint.trait_name,
                    span: constraint.span,
                });
            }
        }
        Ok(())
    }

    fn has_impl(&self, ty: &Ty, trait_name: Symbol) -> bool {
        self.registry.impls.contains_key(&(trait_name, ty.clone()))
    }
}
```

**Type-check method calls:**
```rust
Expr::MethodCall { receiver, method, args, id, span } => {
    let receiver_ty = self.infer_expr(receiver)?;

    // Look up method in traits
    for (trait_name, trait_def) in &self.registry.traits {
        if let Some(method_sig) = trait_def.methods.get(method) {
            // Check if receiver type implements this trait
            if self.has_impl(&receiver_ty, *trait_name) {
                // Type-check arguments
                let arg_tys: Vec<_> = args.iter()
                    .map(|arg| self.infer_expr(arg))
                    .collect::<Result<_, _>>()?;

                // Unify with method signature
                for (arg_ty, param_ty) in arg_tys.iter().zip(&method_sig.params) {
                    self.unify(arg_ty, param_ty, *span)?;
                }

                let ret_ty = method_sig.ret.clone();
                self.node_types.insert(*id, ret_ty.clone());
                return Ok(ret_ty);
            }
        }
    }

    Err(TypeError::NoSuchMethod {
        ty: receiver_ty,
        method: *method,
        span: *span,
    })
}
```

**Type-check generic functions:**
```rust
fn infer_fn_def(&mut self, fn_def: &FnDef) -> Result<(), TypeError> {
    // Create type variables for type parameters
    let mut type_param_map = HashMap::new();
    for type_param in &fn_def.type_params {
        let ty_var = self.fresh_tyvar();
        type_param_map.insert(type_param.name, ty_var.clone());

        // Add trait bound constraints
        for bound in &type_param.bounds {
            self.add_constraint(ty_var.clone(), *bound, type_param.span);
        }
    }

    // Type-check function body with type parameters in scope
    // ...

    // Check all constraints
    self.check_constraints()?;

    Ok(())
}
```

### 5. ferric_compiler Additions

No major changes needed - method calls compile to regular function calls with the receiver as the first argument.

```rust
Expr::MethodCall { receiver, method, args, .. } => {
    // Compile receiver
    self.compile_expr(receiver);

    // Compile arguments
    for arg in args {
        self.compile_expr(arg);
    }

    // Look up which function to call based on receiver type
    // This requires type information
    let receiver_ty = &self.types.node_types[&receiver.id()];
    let impl_def = self.find_impl(receiver_ty, method);

    // Call the impl function
    let fn_idx = self.resolve.fn_slots[&impl_def.method_def_id];
    self.emit(Op::Call((args.len() + 1) as u8));  // +1 for receiver
}
```

### 6. ferric_vm Additions

No changes needed - method calls are just function calls with an extra argument.

## Integration in main.rs

```rust
// After resolution:
let resolve_result = ferric_resolve::resolve(&parse_result);

// NEW: Build trait registry
let trait_registry = ferric_traits::build_registry(&parse_result, &interner);

// Type check with trait registry
let type_result = ferric_infer::typecheck(
    &parse_result,
    &resolve_result,
    &trait_registry,  // NEW argument
);

// Rest unchanged
```

## New Error Types

Add to `TypeError`:
```rust
pub enum TypeError {
    // ... existing variants

    TraitNotImplemented {
        ty: Ty,
        trait_name: Symbol,
        span: Span,
    },

    NoSuchMethod {
        ty: Ty,
        method: Symbol,
        span: Span,
    },

    TraitBoundNotSatisfied {
        type_param: Symbol,
        bound: Symbol,
        span: Span,
    },
}
```

## Test Cases

1. Trait definition parses correctly
2. Impl block parses correctly
3. Generic function with trait bounds parses correctly
4. Method call on type that implements trait type-checks
5. Method call on type that doesn't implement trait produces error
6. Generic function with satisfied trait bounds type-checks
7. Generic function with unsatisfied trait bounds produces error
8. Built-in trait impls (Display for Int, etc.) work
9. All M1-M4 programs still work
10. Complex example with multiple traits and impls works

## Acceptance Criteria

- [ ] Trait and impl parsing works
- [ ] `ferric_traits` crate builds trait registry
- [ ] Type checker extended with trait constraint solving
- [ ] Generic functions with trait bounds work
- [ ] Method calls resolve to correct impl
- [ ] Trait bound errors are clear and helpful
- [ ] Built-in trait impls registered at startup
- [ ] All M1-M4 tests still pass
- [ ] Integration requires **one import swap + one new stage call + one new argument**
- [ ] No changes to lexer, parser, resolver, compiler, VM, diagnostics, or stdlib (except adding trait-based methods)

## Critical Architecture Validation

**This milestone proves the signature can evolve:**
- Type checker signature extended with one parameter
- `main.rs` changes: import swap + new stage call + new argument
- All other stages untouched
- The architecture handles evolutionary changes gracefully

Document this success in the replacement log.

## Notes for Agent
- This is complex - trait constraint solving is non-trivial
- Start with simple trait definitions and impls
- Test each feature independently before combining
- Make sure generic functions work before adding trait bounds
- The trait registry is a new shared dependency - design it carefully
- Consider using a constraint solver library if available in Rust
- Document the trait resolution algorithm clearly
