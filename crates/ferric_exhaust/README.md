# ferric_exhaust

Exhaustiveness checking for `match` expressions. Stage 9 of the pipeline.

## Entry point

```rust
pub fn check_exhaustiveness(ast: &ParseResult, types: &TypeResult) -> ExhaustivenessResult
```

`ExhaustivenessResult` contains a `Vec<ExhaustivenessError>`.

## What it checks

**Non-exhaustive matches** — a `match` on an enum type that doesn't cover all variants produces a `NonExhaustive` error listing the missing variant names. Wildcard (`_`) and variable patterns count as catch-all coverage.

**Unreachable arms** — an arm that appears after an irrefutable pattern (wildcard, variable binding) can never execute. These are reported as `UnreachableArm` with the span of the dead arm.

## Error types

```rust
NonExhaustive { missing: Vec<Symbol>, span: Span }
UnreachableArm { span: Span }
```

Both errors carry a `Span` so `ferric_diagnostics` can render them with source context.
