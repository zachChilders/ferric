# Task: M4 Algebraic Data Types

## Objective
Add structs, enums, and pattern matching to the language. Introduce exhaustiveness checking as a new pipeline stage. This milestone adds features **without replacing any stages**.

## Architecture Context
- This demonstrates **additive changes** plus **one new stage**
- New stage: `ferric_exhaust` for exhaustiveness checking
- All existing stages extend to support ADTs
- No stage replacements - only additions and one insertion

## M4 Target Program

```rust
enum Shape {
    Circle(Float),
    Rectangle(Float, Float),
}

fn area(s: Shape) -> Float {
    match s {
        Shape::Circle(r) => 3.14159 * r * r,
        Shape::Rectangle(w, h) => w * h,
    }
}

struct Point { x: Float, y: Float }
let p = Point { x: 1.0, y: 2.0 }
println(p.x)
```

## Changes to ferric_common

### Extend Ty enum
```rust
pub enum Ty {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    Fn { params: Vec<Ty>, ret: Box<Ty> },
    Var(TyVar),
    Tuple(Vec<Ty>),  // NEW
    Struct {         // NEW
        def_id: DefId,
        fields: Vec<(Symbol, Ty)>,
    },
    Enum {           // NEW
        def_id: DefId,
        variants: Vec<(Symbol, Vec<Ty>)>,
    },
}
```

### Add to Item enum
```rust
pub enum Item {
    FnDef { ... },  // existing

    StructDef {     // NEW
        id: NodeId,
        name: Symbol,
        fields: Vec<(Symbol, TypeAnnotation)>,
        span: Span,
    },

    EnumDef {       // NEW
        id: NodeId,
        name: Symbol,
        variants: Vec<(Symbol, Vec<TypeAnnotation>)>,
        span: Span,
    },
}
```

### Add to Expr enum
```rust
pub enum Expr {
    // ... existing variants

    StructLit {     // NEW: Point { x: 1.0, y: 2.0 }
        name: Symbol,
        fields: Vec<(Symbol, Expr)>,
        id: NodeId,
        span: Span,
    },

    FieldAccess {   // NEW: p.x
        expr: Box<Expr>,
        field: Symbol,
        id: NodeId,
        span: Span,
    },

    Match {         // NEW: match x { ... }
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        id: NodeId,
        span: Span,
    },

    Tuple {         // NEW: (1, 2, 3)
        elements: Vec<Expr>,
        id: NodeId,
        span: Span,
    },
}

pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

pub enum Pattern {
    Wildcard { span: Span },                                    // _
    Variable { name: Symbol, span: Span },                      // x
    Literal { value: Literal, span: Span },                     // 42, "hello"
    Tuple { patterns: Vec<Pattern>, span: Span },               // (x, y, z)
    Struct { name: Symbol, fields: Vec<(Symbol, Pattern)>, span: Span },  // Point { x, y }
    Variant { name: Symbol, variant: Symbol, patterns: Vec<Pattern>, span: Span }, // Shape::Circle(r)
}
```

## Stage Changes

### 1. ferric_lexer Additions

Add keywords:
```rust
"struct" => TokenKind::Struct,
"enum" => TokenKind::Enum,
"match" => TokenKind::Match,
```

Add operators:
```rust
"." => TokenKind::Dot,
"::" => TokenKind::ColonColon,
"_" => TokenKind::Underscore,
```

### 2. ferric_parser Additions

**Struct definition:**
```rust
fn parse_struct_def(&mut self) -> Item {
    self.expect(TokenKind::Struct, "expected 'struct'");
    let name = self.expect_ident();
    self.expect(TokenKind::LBrace, "expected '{'");

    let mut fields = vec![];
    while !self.check(TokenKind::RBrace) {
        let field_name = self.expect_ident();
        self.expect(TokenKind::Colon, "expected ':'");
        let ty = self.parse_type();
        fields.push((field_name, ty));

        if !self.check(TokenKind::RBrace) {
            self.expect(TokenKind::Comma, "expected ','");
        }
    }

    self.expect(TokenKind::RBrace, "expected '}'");
    Item::StructDef { ... }
}
```

**Enum definition:**
```rust
fn parse_enum_def(&mut self) -> Item {
    self.expect(TokenKind::Enum, "expected 'enum'");
    let name = self.expect_ident();
    self.expect(TokenKind::LBrace, "expected '{'");

    let mut variants = vec![];
    while !self.check(TokenKind::RBrace) {
        let variant_name = self.expect_ident();
        let mut types = vec![];

        if self.check(TokenKind::LParen) {
            self.advance();
            while !self.check(TokenKind::RParen) {
                types.push(self.parse_type());
                if !self.check(TokenKind::RParen) {
                    self.expect(TokenKind::Comma, "expected ','");
                }
            }
            self.expect(TokenKind::RParen, "expected ')'");
        }

        variants.push((variant_name, types));

        if !self.check(TokenKind::RBrace) {
            self.expect(TokenKind::Comma, "expected ','");
        }
    }

    self.expect(TokenKind::RBrace, "expected '}'");
    Item::EnumDef { ... }
}
```

**Struct literal:**
```rust
Expr::StructLit {
    name,
    fields: vec![(field_name, field_expr), ...],
    ...
}
```

**Field access:**
```rust
fn parse_postfix_expr(&mut self) -> Expr {
    let mut expr = self.parse_primary();

    loop {
        if self.check(TokenKind::Dot) {
            self.advance();
            let field = self.expect_ident();
            expr = Expr::FieldAccess {
                expr: Box::new(expr),
                field,
                id: self.node_id_gen.next(),
                span: ...
            };
        } else if self.check(TokenKind::LParen) {
            // function call
            ...
        } else {
            break;
        }
    }

    expr
}
```

**Match expression:**
```rust
fn parse_match(&mut self) -> Expr {
    self.expect(TokenKind::Match, "expected 'match'");
    let scrutinee = self.parse_expr();
    self.expect(TokenKind::LBrace, "expected '{'");

    let mut arms = vec![];
    while !self.check(TokenKind::RBrace) {
        let pattern = self.parse_pattern();
        self.expect(TokenKind::Arrow, "expected '=>'");
        let body = self.parse_expr();
        arms.push(MatchArm { pattern, body, span: ... });

        if !self.check(TokenKind::RBrace) {
            self.expect(TokenKind::Comma, "expected ','");
        }
    }

    self.expect(TokenKind::RBrace, "expected '}'");
    Expr::Match { scrutinee: Box::new(scrutinee), arms, ... }
}

fn parse_pattern(&mut self) -> Pattern {
    match self.peek().kind {
        TokenKind::Underscore => {
            self.advance();
            Pattern::Wildcard { ... }
        }
        TokenKind::Ident(name) => {
            // Could be variable, struct pattern, or variant pattern
            // Peek ahead for :: or {
            ...
        }
        TokenKind::IntLit(n) => {
            Pattern::Literal { value: Literal::Int(n), ... }
        }
        TokenKind::LParen => {
            // Tuple pattern
            self.parse_tuple_pattern()
        }
        _ => {
            self.error("expected pattern");
            Pattern::Wildcard { ... }
        }
    }
}
```

### 3. ferric_resolve Additions

**Track struct and enum definitions:**
```rust
struct Resolver {
    // ... existing fields
    type_defs: HashMap<Symbol, DefId>,  // struct/enum definitions
    field_indices: HashMap<DefId, HashMap<Symbol, usize>>,  // struct fields
    variant_indices: HashMap<DefId, HashMap<Symbol, usize>>,  // enum variants
}
```

**Resolve struct/enum definitions:**
```rust
fn resolve_item(&mut self, item: &Item) {
    match item {
        Item::StructDef { name, fields, .. } => {
            let def_id = self.def_id_gen.next();
            self.type_defs.insert(*name, def_id);

            let mut field_map = HashMap::new();
            for (i, (field_name, _)) in fields.iter().enumerate() {
                field_map.insert(*field_name, i);
            }
            self.field_indices.insert(def_id, field_map);
        }

        Item::EnumDef { name, variants, .. } => {
            let def_id = self.def_id_gen.next();
            self.type_defs.insert(*name, def_id);

            let mut variant_map = HashMap::new();
            for (i, (variant_name, _)) in variants.iter().enumerate() {
                variant_map.insert(*variant_name, i);
            }
            self.variant_indices.insert(def_id, variant_map);
        }

        // ... existing cases
    }
}
```

**Resolve field access:**
```rust
Expr::FieldAccess { expr, field, id, .. } => {
    self.resolve_expr(expr);
    // Type checker will verify field exists
}
```

**Resolve patterns:**
```rust
fn resolve_pattern(&mut self, pattern: &Pattern) {
    match pattern {
        Pattern::Variable { name, .. } => {
            let def_id = self.define(*name, false, pattern.span());
            // Bind pattern variable
        }
        Pattern::Struct { name, fields, .. } => {
            // Look up struct definition
            if let Some(def_id) = self.type_defs.get(name) {
                // Verify all fields exist
                for (field_name, field_pat) in fields {
                    self.resolve_pattern(field_pat);
                }
            } else {
                self.errors.push(ResolveError::UndefinedType { ... });
            }
        }
        Pattern::Variant { name, variant, patterns, .. } => {
            // Similar to struct
        }
        // ... other patterns
    }
}
```

### 4. ferric_infer Additions

**Type-check struct literals:**
```rust
Expr::StructLit { name, fields, id, span } => {
    // Look up struct definition
    let def_id = self.resolve.type_defs[name];
    let struct_def = &self.ast.find_struct(def_id);

    // Check all fields present and types match
    for (field_name, field_ty) in &struct_def.fields {
        if let Some((_, field_expr)) = fields.iter().find(|(n, _)| n == field_name) {
            let expr_ty = self.infer_expr(field_expr)?;
            self.unify(&field_ty, &expr_ty, *span)?;
        } else {
            self.errors.push(TypeError::MissingField { ... });
        }
    }

    let struct_ty = Ty::Struct { def_id, fields: struct_def.fields.clone() };
    self.node_types.insert(*id, struct_ty.clone());
    Ok(struct_ty)
}
```

**Type-check field access:**
```rust
Expr::FieldAccess { expr, field, id, span } => {
    let expr_ty = self.infer_expr(expr)?;
    match expr_ty {
        Ty::Struct { def_id, fields } => {
            if let Some((_, field_ty)) = fields.iter().find(|(n, _)| n == field) {
                self.node_types.insert(*id, field_ty.clone());
                Ok(field_ty.clone())
            } else {
                Err(TypeError::NoSuchField { ... })
            }
        }
        _ => Err(TypeError::NotAStruct { ... }),
    }
}
```

**Type-check match:**
```rust
Expr::Match { scrutinee, arms, id, span } => {
    let scrutinee_ty = self.infer_expr(scrutinee)?;

    let mut arm_tys = vec![];
    for arm in arms {
        // Check pattern matches scrutinee type
        self.check_pattern(&arm.pattern, &scrutinee_ty)?;
        let body_ty = self.infer_expr(&arm.body)?;
        arm_tys.push(body_ty);
    }

    // All arms must have same type
    let first_ty = &arm_tys[0];
    for ty in &arm_tys[1..] {
        self.unify(first_ty, ty, *span)?;
    }

    self.node_types.insert(*id, first_ty.clone());
    Ok(first_ty.clone())
}
```

### 5. NEW STAGE: ferric_exhaust

**Create exhaustiveness checking crate:**

```rust
// ferric_exhaust/src/lib.rs
pub fn check_exhaustiveness(ast: &ParseResult, types: &TypeResult) -> ExhaustivenessResult;

pub struct ExhaustivenessResult {
    pub errors: Vec<ExhaustivenessError>,
}

pub enum ExhaustivenessError {
    NonExhaustive { missing: Vec<String>, span: Span },
    UnreachableArm { span: Span },
}
```

**Algorithm:**
1. For each match expression, get the scrutinee type
2. If it's an enum, compute all possible variants
3. Check that patterns cover all variants
4. Detect unreachable patterns (patterns after wildcard)

**Example implementation:**
```rust
fn check_match(scrutinee_ty: &Ty, arms: &[MatchArm]) -> Vec<ExhaustivenessError> {
    match scrutinee_ty {
        Ty::Enum { variants, .. } => {
            let mut covered = HashSet::new();
            let mut errors = vec![];
            let mut seen_wildcard = false;

            for arm in arms {
                if seen_wildcard {
                    errors.push(ExhaustivenessError::UnreachableArm { span: arm.span });
                    continue;
                }

                match &arm.pattern {
                    Pattern::Wildcard { .. } => {
                        seen_wildcard = true;
                    }
                    Pattern::Variant { variant, .. } => {
                        covered.insert(*variant);
                    }
                    _ => {}
                }
            }

            // Check if all variants covered
            let all_variants: HashSet<_> = variants.iter().map(|(name, _)| *name).collect();
            let missing: Vec<_> = all_variants.difference(&covered).collect();

            if !missing.is_empty() && !seen_wildcard {
                errors.push(ExhaustivenessError::NonExhaustive {
                    missing: missing.iter().map(|s| format!("{:?}", s)).collect(),
                    span: arms[0].span,
                });
            }

            errors
        }
        _ => vec![],  // Non-enum types don't need exhaustiveness checking
    }
}
```

### 6. ferric_compiler Additions

Add instructions to `Op` enum:
```rust
pub enum Op {
    // ... existing ops

    MakeStruct(u8),      // Create struct with u8 fields (pops fields from stack)
    GetField(u8),        // Get field at index u8 from struct on stack
    MakeVariant(u16, u8), // Create enum variant (u16 = variant index, u8 = field count)
    MatchVariant(u16),   // Check if top of stack is variant u16
    UnpackVariant,       // Unpack variant fields onto stack
    MakeTuple(u8),       // Create tuple (already in M3)
    GetTupleField(u8),   // Get tuple field at index
}
```

**Compile struct literal:**
```rust
Expr::StructLit { fields, .. } => {
    // Push all field values onto stack
    for (_, field_expr) in fields {
        self.compile_expr(field_expr);
    }
    // Create struct
    self.emit(Op::MakeStruct(fields.len() as u8));
}
```

**Compile match:**
```rust
Expr::Match { scrutinee, arms, .. } => {
    self.compile_expr(scrutinee);

    let mut end_jumps = vec![];

    for arm in arms {
        // Duplicate scrutinee for pattern matching
        self.emit(Op::Dup);

        // Compile pattern matching
        let next_arm_jump = self.compile_pattern(&arm.pattern);

        // Compile arm body
        self.compile_expr(&arm.body);

        // Jump to end
        let end_jump = self.emit_jump(Op::Jump(0));
        end_jumps.push(end_jump);

        // Patch jump to next arm
        if let Some(jump) = next_arm_jump {
            self.patch_jump(jump);
        }
    }

    // Patch all end jumps
    for jump in end_jumps {
        self.patch_jump(jump);
    }
}
```

### 7. ferric_vm Additions

Add to Value enum:
```rust
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    Fn(u16),
    Struct(Vec<Value>),            // NEW: fields as array
    Variant(u16, Vec<Value>),      // NEW: (variant_index, fields)
    Tuple(Vec<Value>),             // NEW
}

impl Value {
    pub fn new_struct(fields: Vec<Value>) -> Self { Value::Struct(fields) }
    pub fn new_variant(idx: u16, fields: Vec<Value>) -> Self { Value::Variant(idx, fields) }
    pub fn new_tuple(elements: Vec<Value>) -> Self { Value::Tuple(elements) }
}
```

**Execute new instructions:**
```rust
Op::MakeStruct(field_count) => {
    let mut fields = vec![];
    for _ in 0..*field_count {
        fields.push(self.stack.pop().unwrap());
    }
    fields.reverse();
    self.stack.push(Value::new_struct(fields));
}

Op::GetField(idx) => {
    let value = self.stack.pop().unwrap();
    match value {
        Value::Struct(fields) => {
            self.stack.push(fields[*idx as usize].clone());
        }
        _ => return Err(RuntimeError::NotAStruct { ... }),
    }
}

Op::MakeVariant(variant_idx, field_count) => {
    let mut fields = vec![];
    for _ in 0..*field_count {
        fields.push(self.stack.pop().unwrap());
    }
    fields.reverse();
    self.stack.push(Value::new_variant(*variant_idx, fields));
}

// ... similar for other ops
```

## Integration in main.rs

Add exhaustiveness checking between type checking and compilation:

```rust
// After type checking:
let type_result = ferric_infer::typecheck(&parse_result, &resolve_result);

// NEW: exhaustiveness checking
let exhaust_result = ferric_exhaust::check_exhaustiveness(&parse_result, &type_result);

// Report errors including exhaustiveness errors
if report_errors(..., &exhaust_result) {
    std::process::exit(1);
}

// Compile
let program = ferric_compiler::compile(&parse_result, &resolve_result, &type_result);
```

## Test Cases

1. Struct definition and literal construction works
2. Field access on struct works
3. Enum definition and variant construction works
4. Match on enum with all variants covered compiles
5. Match on enum missing variant produces exhaustiveness error
6. Match with unreachable arm produces warning
7. Pattern matching with wildcards works
8. Nested patterns work
9. Tuple construction and destructuring works
10. All M1-M3 programs still work

## Acceptance Criteria

- [ ] Struct and enum definitions parse correctly
- [ ] Struct literals and field access work
- [ ] Enum variants and pattern matching work
- [ ] Exhaustiveness checking catches non-exhaustive matches
- [ ] Exhaustiveness checking detects unreachable arms
- [ ] New `ferric_exhaust` stage integrated into pipeline
- [ ] All new bytecode instructions implemented
- [ ] All M1-M3 tests still pass
- [ ] Integration requires **one new stage call in main.rs**
- [ ] No stage replacements - only additions

## Notes for Agent
- This is a large feature - break it down and test incrementally
- Start with structs (simpler), then enums, then pattern matching
- Exhaustiveness checking is complex - use a well-tested algorithm
- Make sure pattern variable bindings work correctly
- Test nested patterns thoroughly
- Update diagnostics to handle new error types
