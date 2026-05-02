# ferric_vm

Stack-based bytecode virtual machine. Stage 11 (final execution stage) of the pipeline.

## Executor trait

The VM is accessed only through the `Executor` trait:

```rust
pub trait Executor {
    fn run(
        &mut self,
        program: Program,
        natives: NativeRegistry,
        interner: &Interner,
    ) -> Result<Value, RuntimeError>;
}
```

`BytecodeVM` is the only public implementation. `main.rs` instantiates it by name; everything else holds `Box<dyn Executor>`.

## Runtime values

```rust
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    Fn(u16),                              // index into Program::chunks
    NativeFn(Symbol),                     // dispatched through NativeRegistry
    Tuple(Vec<Value>),
    Array(Vec<Value>),
    Struct(Vec<Value>),                   // fields in definition order
    Variant(u16, Vec<Value>),             // discriminant + payload
    ShellOutput(ferric_common::ShellOutput),
    Closure { fn_idx: u16, captures: Vec<Value> },
    Async(AsyncCell),                     // Arc<Mutex<AsyncState>>
    Handle(u64),                          // opaque token for spawned tasks
}
```

**`Value` is never constructed outside `ferric_vm` by direct enum construction.** Use the `Value::new_*` constructors so the internal representation can change without touching the rest of the project.

## Async scheduler

`spawn` moves an `Async<T>` value onto a worker thread and returns a `Handle`. `join` blocks until two handles complete. `block_on` drives an `Async<T>` from synchronous code. The scheduler is embedded in `BytecodeVM` and uses `std::thread` under the hood — no external async runtime dependency.
