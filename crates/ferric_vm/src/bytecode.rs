//! Bytecode VM. Implements [`crate::Executor`].
//!
//! Classic fetch–decode–execute loop over `Program.chunks`. Each call frame
//! owns its own `slots` vector (local variables indexed by `u8`). The value
//! stack is shared across frames; a caller pushes args then a callable, and
//! `Op::Call(argc)` pops them into a new frame.
//!
//! ## Frame management
//!
//! `CallFrame` lives on a heap-allocated `Vec<CallFrame>` rather than the
//! Rust call stack. This is deliberate — it lets a future async runtime
//! suspend/resume Ferric-level function calls without fighting the borrow
//! checker or blocking the executor. See `ASYNC_COMPAT.md`.

use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use ferric_common::{
    Chunk, Constant, EffectTags, HandlerTable, Interner, Op, Program, ShellOutput, Span, Symbol,
};
use ferric_stdlib::{NativeRegistry, NativeValue};

use crate::{AsyncCell, AsyncState, Executor, RuntimeError, Value};

/// Call frame for a single Ferric function invocation.
#[derive(Clone)]
struct CallFrame {
    /// Index into `Program.chunks` of the code we're executing.
    chunk_idx: u16,
    /// Instruction pointer into `chunks[chunk_idx].code`.
    ip: usize,
    /// Local slots, indexed by the `u8` operand of `LoadSlot`/`StoreSlot`.
    /// Grown on demand by `StoreSlot` (see the opcode handler).
    slots: Vec<Value>,
}

/// One frame on the VM's handler stack. Installed by `Op::PushHandler` and
/// removed by `Op::PopHandler` (or stolen by `Op::Perform` when one of its
/// op handlers is dispatched).
#[derive(Clone)]
struct HandlerFrame {
    /// Effect-tag this frame handles. Multiple frames for the same effect
    /// can coexist; the top-most match wins.
    effect_tag: u16,
    /// `op_tag → clause chunk index`. The clause chunk's leading slots are
    /// bound to the operation's argument values (declaration order),
    /// followed by the continuation as the last leading slot.
    op_handlers: HashMap<u8, u16>,
    /// Index of the call frame that pushed this handler in the call_stack
    /// at PushHandler time (i.e. `call_stack.len() - 1`). When a `Perform`
    /// matches this handler, this frame is *kept in place* (its IP is
    /// fast-forwarded past `PopHandler`) and any frames *above* it are
    /// captured into the continuation.
    frame_depth: usize,
    /// Length of the value stack at the moment this frame was installed.
    /// Operands above this depth are part of the suspended computation
    /// when a `Perform` matches this frame.
    stack_depth: usize,
    /// Length of the handler_stack *below* this frame (i.e. its own index
    /// before insertion). Handlers above this depth are nested inside the
    /// suspended computation and are captured into the continuation.
    handler_depth: usize,
    /// IP within the handler-installing frame's chunk to fast-forward
    /// to when a `Perform` against this handler is captured. Computed
    /// at compile time as the instruction immediately after the matching
    /// `PopHandler`. If the matched clause never resumes, the
    /// handler-installing frame continues from this IP — the clause's
    /// return value flows out as the handle expression's result.
    body_end_ip: usize,
}

/// Captured suspended state from an `Op::Perform` match. The id this is
/// stored under is wrapped in a `Value::Continuation` and handed to the
/// matched handler clause; running `Op::Resume` on that id restores the
/// state and continues from the perform site. One-shot only — slot is
/// `None` after consumption.
#[derive(Clone)]
struct Continuation {
    /// Operand-stack slice that lived above the handler frame at perform
    /// time. Restored verbatim on resume.
    stack_above: Vec<Value>,
    /// Call frames that lived strictly *above* the handler-installing
    /// frame at perform time (i.e. nested calls made during body
    /// evaluation). Restored on resume so the suspended body can
    /// continue. The handler-installing frame itself is **not** in this
    /// vector — it stays in place on the call_stack and has its IP
    /// swapped between `body_end_ip` (suspended) and
    /// `handler_frame_ip` (resumed).
    frames_above: Vec<CallFrame>,
    /// Handler frames that were nested above the matching handler at
    /// perform time. Restored on resume — they protect re-emitted effects
    /// inside the resumed body.
    handlers_above: Vec<HandlerFrame>,
    /// The handler frame that matched. Re-installed on resume so the
    /// resumed body can re-perform the same effect against the same
    /// handler.
    matched_frame: HandlerFrame,
    /// IP within the handler-installing frame at perform time (the
    /// instruction immediately after the `Op::Perform`). On resume this
    /// is swapped back into the handler-installing frame's `ip`,
    /// replacing the `body_end_ip` that `op_perform` set.
    handler_frame_ip: usize,
}

/// One spawned task's status as held in `Scheduler::handles`.
enum TaskState {
    /// Worker thread still running. The `JoinHandle` is `take()`n the first
    /// time someone awaits the corresponding `Handle`, so this state can
    /// transition exactly once into `Ready`.
    Running(thread::JoinHandle<Result<Value, RuntimeError>>),
    /// Task completed; cached value available for repeated awaits.
    Ready(Value),
}

/// Cooperative scheduler shared across all `BytecodeVM` instances spawned
/// for tasks. Each `spawn` mints a fresh task id, kicks off a worker
/// thread, and inserts a `TaskState::Running` entry. `handle.await` /
/// `block_on` / `join` look up entries here, joining the worker thread
/// (blocking) the first time and caching the resolved value.
pub(crate) struct Scheduler {
    task_seq: AtomicU64,
    handles: Mutex<HashMap<u64, TaskState>>,
}

impl Scheduler {
    fn new() -> Self {
        Self {
            task_seq: AtomicU64::new(0),
            handles: Mutex::new(HashMap::new()),
        }
    }

    fn next_id(&self) -> u64 {
        self.task_seq.fetch_add(1, Ordering::Relaxed)
    }
}

/// Symbols of the M8 async stdlib intrinsics. Resolved once at the start
/// of `run` so the VM can dispatch by Symbol rather than re-resolving the
/// interner string on every call.
#[derive(Default, Clone, Copy)]
struct AsyncIntrinsics {
    spawn: Option<Symbol>,
    join: Option<Symbol>,
    sleep: Option<Symbol>,
    shell_run_async: Option<Symbol>,
    /// `block_on(task: Async<T>) -> T` — host-side runner that drives an
    /// async to completion synchronously. Legal at the top level where
    /// `await` is forbidden.
    block_on: Option<Symbol>,
}

impl AsyncIntrinsics {
    fn from_interner(interner: &Interner) -> Self {
        Self {
            spawn: interner.lookup("spawn"),
            join: interner.lookup("join"),
            sleep: interner.lookup("sleep"),
            shell_run_async: interner.lookup("shell_run_async"),
            block_on: interner.lookup("block_on"),
        }
    }
}

/// Pre-resolved tag pairs for built-in effects. Populated at the start of
/// [`Executor::run`] from the program's [`EffectTags`] table — the effects
/// pass assigns these tags lazily, so they are not stable across programs.
///
/// Built-in effects are dispatched directly by the VM rather than through a
/// user-installed handler clause, because their semantics depend on runtime
/// state (the async scheduler, generator collectors) that lives outside the
/// bytecode model. Each `Op::Perform` site checks these tag pairs first and
/// short-circuits to the matching VM-side handler when one matches; otherwise
/// it falls through to the regular handler-stack walk.
#[derive(Default, Clone, Copy)]
struct BuiltinEffectTags {
    /// `(effect_tag, op_tag)` for `Async::Suspend`. `None` if the program
    /// never references it (pure M1–M7 sync programs).
    async_suspend: Option<(u16, u8)>,
}

impl BuiltinEffectTags {
    fn from_tags(tags: &EffectTags, interner: &Interner) -> Self {
        let async_suspend = match (interner.lookup("Async"), interner.lookup("Suspend")) {
            (Some(eff), Some(op)) => tags.lookup(eff, op),
            _ => None,
        };
        Self { async_suspend }
    }
}

/// Bytecode interpreter.
///
/// The execution loop is driven by [`run_chunk_to_completion`] — a re-entrant
/// dispatch loop that runs a single chunk until its frame pops. The public
/// entry point [`Executor::run`] wraps that with the entry chunk; awaiting
/// a `Pending` `Value::Async` re-enters with the body chunk; spawned tasks
/// build a fresh VM (sharing program/interner/scheduler/natives via `Arc`s)
/// and re-enter on a worker thread.
pub struct BytecodeVM {
    stack: Vec<Value>,
    call_stack: Vec<CallFrame>,
    /// M9 algebraic-effects handler stack. Separate from the call stack;
    /// `Op::PushHandler` adds a frame, `Op::PopHandler` removes one, and
    /// `Op::Perform` walks it top-to-bottom to find a matching clause.
    handler_stack: Vec<HandlerFrame>,
    /// Slab of one-shot continuations indexed by `u32`. `None` slots
    /// represent consumed continuations — resuming one is `ResumedTwice`.
    continuations: Vec<Option<Continuation>>,

    /// Program chunks, shared with worker threads.
    chunks: Arc<Vec<Chunk>>,
    /// Handler tables compiled into the program; one per `handle ... with`
    /// expression. `Op::PushHandler(table_idx)` reads from this list.
    handler_tables: Arc<Vec<HandlerTable>>,
    /// Native function registry, shared with worker threads.
    natives: Arc<NativeRegistry>,
    /// Interner, shared with worker threads (read-only after compilation).
    interner: Arc<Interner>,
    /// Cached Symbols for VM-internal stdlib intrinsics.
    intrinsics: AsyncIntrinsics,
    /// Cached tag pairs for built-in effects (`Async::Suspend`). Populated
    /// from the program's `EffectTags` table at the start of `run`.
    builtin_effect_tags: BuiltinEffectTags,
    /// Cooperative scheduler shared across all VMs.
    scheduler: Arc<Scheduler>,
}

impl BytecodeVM {
    /// Creates a new bytecode VM. State is replaced on each call to
    /// [`Executor::run`]; only the empty stack/call_stack are reusable.
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            call_stack: Vec::new(),
            handler_stack: Vec::new(),
            continuations: Vec::new(),
            chunks: Arc::new(Vec::new()),
            handler_tables: Arc::new(Vec::new()),
            natives: Arc::new(NativeRegistry::new()),
            interner: Arc::new(Interner::new()),
            intrinsics: AsyncIntrinsics::default(),
            builtin_effect_tags: BuiltinEffectTags::default(),
            scheduler: Arc::new(Scheduler::new()),
        }
    }

    /// Constructs a worker-thread VM that shares the parent's Arcs but has
    /// its own stack and call_stack. Used by the `spawn` intrinsic.
    fn worker(
        chunks: Arc<Vec<Chunk>>,
        handler_tables: Arc<Vec<HandlerTable>>,
        natives: Arc<NativeRegistry>,
        interner: Arc<Interner>,
        intrinsics: AsyncIntrinsics,
        builtin_effect_tags: BuiltinEffectTags,
        scheduler: Arc<Scheduler>,
    ) -> Self {
        Self {
            stack: Vec::new(),
            call_stack: Vec::new(),
            handler_stack: Vec::new(),
            continuations: Vec::new(),
            chunks,
            handler_tables,
            natives,
            interner,
            intrinsics,
            builtin_effect_tags,
            scheduler,
        }
    }
}

impl Default for BytecodeVM {
    fn default() -> Self {
        Self::new()
    }
}

impl Executor for BytecodeVM {
    fn run(
        &mut self,
        program: Program,
        natives: NativeRegistry,
        interner: &Interner,
    ) -> Result<Value, RuntimeError> {
        self.stack.clear();
        self.call_stack.clear();
        self.handler_stack.clear();
        self.continuations.clear();
        self.chunks = Arc::new(program.chunks);
        self.handler_tables = Arc::new(program.handler_tables);
        self.natives = Arc::new(natives);
        // Cloning the interner here keeps the public `&Interner` signature
        // intact while still letting worker threads hold an Arc<Interner>
        // for symbol lookups.
        self.interner = Arc::new(interner.clone());
        self.intrinsics = AsyncIntrinsics::from_interner(interner);
        self.builtin_effect_tags = BuiltinEffectTags::from_tags(&program.effect_tags, interner);
        self.scheduler = Arc::new(Scheduler::new());

        self.run_chunk_to_completion(program.entry, Vec::new())
    }
}

impl BytecodeVM {
    /// Pushes a frame for `chunk_idx` initialised with `args` as its leading
    /// slots, then drives the dispatch loop until that frame pops. Returns
    /// the top of the stack at exit. Re-entrant: `Op::Await` calls this
    /// recursively to drive a `Pending` `Value::Async` to completion.
    fn run_chunk_to_completion(
        &mut self,
        chunk_idx: u16,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        let entry_depth = self.call_stack.len();
        self.call_stack.push(CallFrame {
            chunk_idx,
            ip: 0,
            slots: args,
        });

        // Cloning the Arc gives us a stable `&Vec<Chunk>` for the duration
        // of the loop without holding an immutable borrow on `self`.
        let chunks = Arc::clone(&self.chunks);

        // Main interpreter loop. Stops when the call_stack returns to the
        // entry depth — the frame this call pushed has popped.
        while self.call_stack.len() > entry_depth {
            let (chunk_idx, ip) = {
                let frame = self.call_stack.last().unwrap();
                (frame.chunk_idx, frame.ip)
            };
            let chunk: &Chunk = &chunks[chunk_idx as usize];

            // Implicit return if we fall off the end of a chunk.
            if ip >= chunk.code.len() {
                self.call_stack.pop();
                continue;
            }

            let op = chunk.code[ip];
            self.call_stack.last_mut().unwrap().ip = ip + 1;

            match op {
                // ---------------- Stack / slots --------------------------
                Op::LoadConst(idx) => {
                    let v = constant_to_value(&chunk.constants[idx as usize]);
                    self.stack.push(v);
                }
                Op::LoadSlot(slot) => {
                    let frame = self.call_stack.last().unwrap();
                    let v = frame
                        .slots
                        .get(slot as usize)
                        .cloned()
                        .unwrap_or(Value::Unit);
                    self.stack.push(v);
                }
                Op::StoreSlot(slot) => {
                    let v = self.pop()?;
                    let frame = self.call_stack.last_mut().unwrap();
                    if (slot as usize) >= frame.slots.len() {
                        frame.slots.resize(slot as usize + 1, Value::Unit);
                    }
                    frame.slots[slot as usize] = v;
                }
                Op::Pop => {
                    self.pop()?;
                }
                Op::Dup => {
                    let top = self.stack.last().cloned().ok_or_else(underflow)?;
                    self.stack.push(top);
                }

                // ---------------- Integer arithmetic ---------------------
                Op::AddInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    let v = a.checked_add(b).ok_or(RuntimeError::IntegerOverflow {
                        op: "+",
                        span: dummy_span(),
                    })?;
                    self.stack.push(Value::new_int(v));
                }
                Op::SubInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    let v = a.checked_sub(b).ok_or(RuntimeError::IntegerOverflow {
                        op: "-",
                        span: dummy_span(),
                    })?;
                    self.stack.push(Value::new_int(v));
                }
                Op::MulInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    let v = a.checked_mul(b).ok_or(RuntimeError::IntegerOverflow {
                        op: "*",
                        span: dummy_span(),
                    })?;
                    self.stack.push(Value::new_int(v));
                }
                Op::DivInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    if b == 0 {
                        return Err(RuntimeError::DivisionByZero { span: dummy_span() });
                    }
                    let v = a.checked_div(b).ok_or(RuntimeError::IntegerOverflow {
                        op: "/",
                        span: dummy_span(),
                    })?;
                    self.stack.push(Value::new_int(v));
                }
                Op::RemInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    if b == 0 {
                        return Err(RuntimeError::DivisionByZero { span: dummy_span() });
                    }
                    let v = a.checked_rem(b).ok_or(RuntimeError::IntegerOverflow {
                        op: "%",
                        span: dummy_span(),
                    })?;
                    self.stack.push(Value::new_int(v));
                }
                Op::NegInt => {
                    let a = self.pop_int()?;
                    let v = a.checked_neg().ok_or(RuntimeError::IntegerOverflow {
                        op: "-",
                        span: dummy_span(),
                    })?;
                    self.stack.push(Value::new_int(v));
                }

                // ---------------- Float arithmetic -----------------------
                Op::AddFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_float(a + b));
                }
                Op::SubFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_float(a - b));
                }
                Op::MulFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_float(a * b));
                }
                Op::DivFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    if b == 0.0 {
                        return Err(RuntimeError::DivisionByZero { span: dummy_span() });
                    }
                    self.stack.push(Value::new_float(a / b));
                }
                Op::NegFloat => {
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_float(-a));
                }

                // ---------------- Comparisons ----------------------------
                Op::EqInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_bool(a == b));
                }
                Op::NeInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_bool(a != b));
                }
                Op::LtInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_bool(a < b));
                }
                Op::GtInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_bool(a > b));
                }
                Op::LeInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_bool(a <= b));
                }
                Op::GeInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_bool(a >= b));
                }
                Op::EqFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_bool(a == b));
                }
                Op::NeFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_bool(a != b));
                }
                Op::LtFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_bool(a < b));
                }
                Op::GtFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_bool(a > b));
                }
                Op::LeFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_bool(a <= b));
                }
                Op::GeFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_bool(a >= b));
                }
                Op::EqBool => {
                    let b = self.pop_bool()?;
                    let a = self.pop_bool()?;
                    self.stack.push(Value::new_bool(a == b));
                }
                Op::NeBool => {
                    let b = self.pop_bool()?;
                    let a = self.pop_bool()?;
                    self.stack.push(Value::new_bool(a != b));
                }
                Op::EqStr => {
                    let b = self.pop_str()?;
                    let a = self.pop_str()?;
                    self.stack.push(Value::new_bool(a == b));
                }
                Op::NeStr => {
                    let b = self.pop_str()?;
                    let a = self.pop_str()?;
                    self.stack.push(Value::new_bool(a != b));
                }

                // ---------------- Boolean logic --------------------------
                Op::Not => {
                    let a = self.pop_bool()?;
                    self.stack.push(Value::new_bool(!a));
                }
                Op::AndBool => {
                    let b = self.pop_bool()?;
                    let a = self.pop_bool()?;
                    self.stack.push(Value::new_bool(a && b));
                }
                Op::OrBool => {
                    let b = self.pop_bool()?;
                    let a = self.pop_bool()?;
                    self.stack.push(Value::new_bool(a || b));
                }

                // ---------------- Strings --------------------------------
                Op::Concat => {
                    let b = self.pop_str()?;
                    let a = self.pop_str()?;
                    let mut out = String::with_capacity(a.len() + b.len());
                    out.push_str(&a);
                    out.push_str(&b);
                    self.stack.push(Value::new_str(out));
                }

                // ---------------- Control flow ---------------------------
                // Jump offsets are relative to the instruction immediately
                // after the jump — i.e. the `ip` we already advanced to.
                Op::Jump(off) => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.ip = apply_offset(frame.ip, off);
                }
                Op::JumpIfFalse(off) => {
                    let cond = self.pop_bool()?;
                    if !cond {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = apply_offset(frame.ip, off);
                    }
                }
                Op::JumpIfTrue(off) => {
                    let cond = self.pop_bool()?;
                    if cond {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = apply_offset(frame.ip, off);
                    }
                }
                Op::Return => {
                    self.call_stack.pop();
                }

                // ---------------- Calls ----------------------------------
                Op::Call(argc) => {
                    let callee = self.pop()?;
                    let argc = argc as usize;
                    if self.stack.len() < argc {
                        return Err(underflow());
                    }
                    let args_start = self.stack.len() - argc;
                    let args: Vec<Value> = self.stack.drain(args_start..).collect();

                    match callee {
                        Value::Fn(chunk_idx) => {
                            self.call_stack.push(CallFrame {
                                chunk_idx,
                                ip: 0,
                                slots: args,
                            });
                        }
                        Value::Closure { fn_idx, captures } => {
                            // Captures occupy the leading slots of the
                            // closure's frame, followed by the call args.
                            let mut slots = captures;
                            slots.extend(args);
                            self.call_stack.push(CallFrame {
                                chunk_idx: fn_idx,
                                ip: 0,
                                slots,
                            });
                        }
                        Value::NativeFn(sym) => {
                            // M8 Task 5: async stdlib intrinsics need
                            // scheduler state and so are dispatched here
                            // rather than through the regular `NativeFn`
                            // boundary (which is value-only).
                            if Some(sym) == self.intrinsics.spawn {
                                self.intrinsic_spawn(args)?;
                            } else if Some(sym) == self.intrinsics.join {
                                self.intrinsic_join(args)?;
                            } else if Some(sym) == self.intrinsics.sleep {
                                self.intrinsic_sleep(args)?;
                            } else if Some(sym) == self.intrinsics.shell_run_async {
                                self.intrinsic_shell_run_async(args)?;
                            } else if Some(sym) == self.intrinsics.block_on {
                                self.intrinsic_block_on(args)?;
                            } else {
                                let native = self.natives.get(sym).copied().ok_or_else(|| {
                                    RuntimeError::UndefinedFunction {
                                        name: sym,
                                        span: dummy_span(),
                                    }
                                })?;
                                let native_args: Vec<NativeValue> =
                                    args.iter().map(value_to_native).collect();
                                match native(&native_args) {
                                    Ok(result) => self.stack.push(native_to_value(result)),
                                    Err(message) => {
                                        return Err(RuntimeError::NativeFunctionError {
                                            message,
                                            span: dummy_span(),
                                        });
                                    }
                                }
                            }
                        }
                        _ => {
                            return Err(RuntimeError::NotCallable { span: dummy_span() });
                        }
                    }
                }

                // ---------------- Misc -----------------------------------
                Op::Unit => {
                    self.stack.push(Value::new_unit());
                }
                Op::RequireFail => {
                    let msg = self.pop_str()?;
                    let message = if msg.is_empty() { None } else { Some(msg) };
                    return Err(RuntimeError::RequireError {
                        span: dummy_span(),
                        message,
                    });
                }
                Op::RequireWarn => {
                    let msg = self.pop_str()?;
                    let display = if msg.is_empty() {
                        "require condition evaluated to false".to_string()
                    } else {
                        msg
                    };
                    eprintln!("warning: require failed: {display}");
                }
                Op::MustFail => {
                    // M10 Task 4: distinct from RequireFail so diagnostics
                    // can render "unwrap failed: ..." instead of "require
                    // failed: ..." — the user did not write `require`.
                    let msg = self.pop_str()?;
                    let message = if msg.is_empty() { None } else { Some(msg) };
                    return Err(RuntimeError::MustError {
                        span: dummy_span(),
                        message,
                    });
                }

                // ---------------- M4: structs / enums / tuples ------------
                Op::MakeStruct(n) => {
                    let n = n as usize;
                    if self.stack.len() < n {
                        return Err(underflow());
                    }
                    let start = self.stack.len() - n;
                    let fields: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::new_struct(fields));
                }
                Op::GetField(idx) => {
                    let value = self.pop()?;
                    match value {
                        Value::Struct(fields) => {
                            let v = fields.get(idx as usize).cloned().ok_or_else(|| {
                                RuntimeError::InvalidOperation {
                                    op: format!("GetField({idx}) out of range"),
                                    span: dummy_span(),
                                }
                            })?;
                            self.stack.push(v);
                        }
                        v => return Err(type_mismatch("Struct", &v)),
                    }
                }
                Op::MakeVariant(variant_idx, n) => {
                    let n = n as usize;
                    if self.stack.len() < n {
                        return Err(underflow());
                    }
                    let start = self.stack.len() - n;
                    let fields: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::new_variant(variant_idx, fields));
                }
                Op::MatchVariant(idx) => {
                    let value = self.pop()?;
                    match value {
                        Value::Variant(v, _) => {
                            self.stack.push(Value::new_bool(v == idx));
                        }
                        v => return Err(type_mismatch("Variant", &v)),
                    }
                }
                Op::UnpackVariant => {
                    let value = self.pop()?;
                    match value {
                        Value::Variant(_, fields) => {
                            for f in fields {
                                self.stack.push(f);
                            }
                        }
                        v => return Err(type_mismatch("Variant", &v)),
                    }
                }
                Op::MakeTuple(n) => {
                    let n = n as usize;
                    if self.stack.len() < n {
                        return Err(underflow());
                    }
                    let start = self.stack.len() - n;
                    let elements: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::new_tuple(elements));
                }
                Op::GetTupleField(idx) => {
                    let value = self.pop()?;
                    match value {
                        Value::Tuple(elems) => {
                            let v = elems.get(idx as usize).cloned().ok_or_else(|| {
                                RuntimeError::InvalidOperation {
                                    op: format!("GetTupleField({idx}) out of range"),
                                    span: dummy_span(),
                                }
                            })?;
                            self.stack.push(v);
                        }
                        v => return Err(type_mismatch("Tuple", &v)),
                    }
                }

                // ---------------- M6: arrays / closures ------------------
                Op::MakeArray(n) => {
                    let n = n as usize;
                    if self.stack.len() < n {
                        return Err(underflow());
                    }
                    let start = self.stack.len() - n;
                    let elements: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::new_array(elements));
                }
                Op::ArrayGet => {
                    let index = self.pop_int()?;
                    let array = self.pop()?;
                    match array {
                        Value::Array(elements) => {
                            if index < 0 || (index as usize) >= elements.len() {
                                return Err(RuntimeError::IndexOutOfBounds {
                                    index,
                                    len: elements.len(),
                                    span: dummy_span(),
                                });
                            }
                            self.stack.push(elements[index as usize].clone());
                        }
                        v => {
                            return Err(RuntimeError::NotAnArray {
                                found: type_name(&v).to_string(),
                                span: dummy_span(),
                            });
                        }
                    }
                }
                Op::ArrayLen => {
                    let array = self.pop()?;
                    match array {
                        Value::Array(elements) => {
                            self.stack.push(Value::new_int(elements.len() as i64));
                        }
                        v => {
                            return Err(RuntimeError::NotAnArray {
                                found: type_name(&v).to_string(),
                                span: dummy_span(),
                            });
                        }
                    }
                }
                Op::MakeClosure(fn_idx, capture_count) => {
                    let n = capture_count as usize;
                    if self.stack.len() < n {
                        return Err(underflow());
                    }
                    let start = self.stack.len() - n;
                    let captures: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::new_closure(fn_idx, captures));
                }

                // ---------------- M8: async / await ----------------------
                Op::MakeAsync(chunk_idx, n_captures) => {
                    let n = n_captures as usize;
                    if self.stack.len() < n {
                        return Err(underflow());
                    }
                    let start = self.stack.len() - n;
                    let captures: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack
                        .push(Value::new_async_pending(chunk_idx, captures));
                }
                Op::Await => {
                    let v = self.pop()?;
                    let resolved = self.drive_to_value(v)?;
                    self.stack.push(resolved);
                }

                // ---------------- M9: algebraic effects ------------------
                Op::PushHandler(table_idx, body_end_offset) => {
                    self.op_push_handler(table_idx, body_end_offset)?;
                }
                Op::PopHandler => {
                    // Falling out of a `handle { body } with` body normally:
                    // simply drop the top handler frame. The body's value is
                    // already on the value stack and becomes the result of
                    // the handle expression.
                    self.handler_stack
                        .pop()
                        .ok_or_else(|| RuntimeError::InvalidOperation {
                            op: "PopHandler with empty handler stack".to_string(),
                            span: dummy_span(),
                        })?;
                }
                Op::Perform(effect_tag, op_tag, argc) => {
                    self.op_perform(effect_tag, op_tag, argc)?;
                }
                Op::Resume => {
                    self.op_resume()?;
                }
            }
        }

        // Result is whatever sits on top of the value stack at exit.
        Ok(self.stack.pop().unwrap_or(Value::new_unit()))
    }
}

// ============================================================================
// Private helpers
// ============================================================================

impl BytecodeVM {
    fn pop(&mut self) -> Result<Value, RuntimeError> {
        self.stack.pop().ok_or_else(underflow)
    }

    fn pop_int(&mut self) -> Result<i64, RuntimeError> {
        match self.pop()? {
            Value::Int(n) => Ok(n),
            v => Err(type_mismatch("Int", &v)),
        }
    }

    fn pop_float(&mut self) -> Result<f64, RuntimeError> {
        match self.pop()? {
            Value::Float(f) => Ok(f),
            v => Err(type_mismatch("Float", &v)),
        }
    }

    fn pop_bool(&mut self) -> Result<bool, RuntimeError> {
        match self.pop()? {
            Value::Bool(b) => Ok(b),
            v => Err(type_mismatch("Bool", &v)),
        }
    }

    fn pop_str(&mut self) -> Result<String, RuntimeError> {
        match self.pop()? {
            Value::Str(s) => Ok(s),
            v => Err(type_mismatch("Str", &v)),
        }
    }

    // ---------------- M8 async stdlib intrinsics ------------------------
    //
    // Path B Option A (lazy + threaded) — async fn / async block bodies
    // do not run until the resulting `Value::Async` is awaited or spawned.
    // Awaiting drives the body in the current thread; spawning kicks it
    // off on an OS worker thread, enabling actual concurrency.

    /// Drives a `Value::Async` or `Value::Handle` to its inner value.
    /// Centralised here so `Op::Await`, `block_on`, and the per-handle
    /// path of `join` share one implementation. For Pending Async the
    /// body chunk runs in this thread via the re-entrant
    /// `run_chunk_to_completion`; for Running Handle the worker thread
    /// is joined (blocking).
    fn drive_to_value(&mut self, v: Value) -> Result<Value, RuntimeError> {
        match v {
            Value::Async(cell) => self.drive_async(cell),
            Value::Handle(id) => self.drive_handle(id),
            other => Err(type_mismatch("Async or Handle", &other)),
        }
    }

    fn drive_async(&mut self, cell: AsyncCell) -> Result<Value, RuntimeError> {
        // Hold the lock just long enough to inspect (and possibly take) the
        // pending state. Running the chunk re-entrantly inside the lock
        // would deadlock if the body itself awaited the same value.
        let pending = {
            let mut state = cell.lock();
            match &*state {
                AsyncState::Ready(v) => return Ok(v.clone()),
                AsyncState::Pending { .. } => {}
            }
            // Replace with a temporary Ready(Unit) sentinel while we run;
            // we'll write the real Ready value back when the body completes.
            let taken = std::mem::replace(&mut *state, AsyncState::Ready(Value::Unit));
            match taken {
                AsyncState::Pending {
                    chunk_idx,
                    captures,
                } => (chunk_idx, captures),
                AsyncState::Ready(v) => return Ok(v),
            }
        };

        let (chunk_idx, captures) = pending;
        let result = self.run_chunk_to_completion(chunk_idx, captures)?;
        // Cache the result so subsequent awaits return immediately.
        *cell.lock() = AsyncState::Ready(result.clone());
        Ok(result)
    }

    fn drive_handle(&mut self, id: u64) -> Result<Value, RuntimeError> {
        // Take the JoinHandle out (if Running), drop the lock, then join
        // the thread without holding the scheduler lock. Re-acquire to
        // cache the resolved value so repeated awaits are O(1).
        let join_handle = {
            let mut handles = self.scheduler.handles.lock().expect("scheduler poisoned");
            match handles.get_mut(&id) {
                None => {
                    return Err(RuntimeError::AsyncDeadlock { span: dummy_span() });
                }
                Some(TaskState::Ready(v)) => return Ok(v.clone()),
                Some(slot @ TaskState::Running(_)) => {
                    let placeholder = TaskState::Ready(Value::Unit);
                    match std::mem::replace(slot, placeholder) {
                        TaskState::Running(jh) => jh,
                        TaskState::Ready(_) => unreachable!(),
                    }
                }
            }
        };

        let value = match join_handle.join() {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(RuntimeError::NativeFunctionError {
                    message: "spawned task panicked".to_string(),
                    span: dummy_span(),
                });
            }
        };

        self.scheduler
            .handles
            .lock()
            .expect("scheduler poisoned")
            .insert(id, TaskState::Ready(value.clone()));
        Ok(value)
    }

    fn intrinsic_spawn(&mut self, args: Vec<Value>) -> Result<(), RuntimeError> {
        let arg = args
            .into_iter()
            .next()
            .ok_or_else(|| RuntimeError::WrongArgumentCount {
                expected: 1,
                found: 0,
                span: dummy_span(),
            })?;
        // Take the Pending state out of the Async cell so the worker thread
        // owns it. If the cell was already Ready (e.g. spawn(async{42})),
        // we just stash the resolved value and skip the thread.
        //
        // M9 fallthrough: after the parser strips `Item::AsyncFn`, async fn
        // calls produce plain values (`Value::Str("...")`) rather than
        // `Value::Async(Pending(...))`. In that case the work is already
        // complete, so we just box it as an immediately-Ready handle —
        // matching the behaviour of `spawn(async { 42 })` where the body
        // had already evaluated.
        let cell = match arg {
            Value::Async(cell) => cell,
            other => {
                let id = self.scheduler.next_id();
                self.scheduler
                    .handles
                    .lock()
                    .expect("scheduler poisoned")
                    .insert(id, TaskState::Ready(other));
                self.stack.push(Value::new_handle(id));
                return Ok(());
            }
        };

        let id = self.scheduler.next_id();

        let pending = {
            let mut state = cell.lock();
            let taken = std::mem::replace(&mut *state, AsyncState::Ready(Value::Unit));
            match taken {
                AsyncState::Ready(v) => {
                    // Already resolved; no thread needed.
                    self.scheduler
                        .handles
                        .lock()
                        .expect("scheduler poisoned")
                        .insert(id, TaskState::Ready(v));
                    self.stack.push(Value::new_handle(id));
                    return Ok(());
                }
                AsyncState::Pending {
                    chunk_idx,
                    captures,
                } => (chunk_idx, captures),
            }
        };

        let (chunk_idx, captures) = pending;
        let chunks = Arc::clone(&self.chunks);
        let handler_tables = Arc::clone(&self.handler_tables);
        let natives = Arc::clone(&self.natives);
        let interner = Arc::clone(&self.interner);
        let intrinsics = self.intrinsics;
        let builtin_effect_tags = self.builtin_effect_tags;
        let scheduler = Arc::clone(&self.scheduler);

        let join_handle = thread::spawn(move || {
            let mut worker = BytecodeVM::worker(
                chunks,
                handler_tables,
                natives,
                interner,
                intrinsics,
                builtin_effect_tags,
                scheduler,
            );
            worker.run_chunk_to_completion(chunk_idx, captures)
        });

        self.scheduler
            .handles
            .lock()
            .expect("scheduler poisoned")
            .insert(id, TaskState::Running(join_handle));
        self.stack.push(Value::new_handle(id));
        Ok(())
    }

    fn intrinsic_join(&mut self, args: Vec<Value>) -> Result<(), RuntimeError> {
        if args.is_empty() {
            return Err(RuntimeError::WrongArgumentCount {
                expected: 2,
                found: 0,
                span: dummy_span(),
            });
        }
        let mut resolved: Vec<Value> = Vec::with_capacity(args.len());
        for arg in args {
            let v = self.drive_to_value(arg)?;
            resolved.push(v);
        }
        self.stack.push(Value::new_tuple(resolved));
        Ok(())
    }

    fn intrinsic_sleep(&mut self, args: Vec<Value>) -> Result<(), RuntimeError> {
        // sleep(ms) is implemented as a synchronous Thread::sleep wrapped in
        // an already-Ready Async<Unit>. When awaited inside a spawned task
        // this still gives parallelism — the spawned worker thread sleeps
        // independently of the main thread and other workers.
        let ms = match args.into_iter().next() {
            Some(Value::Int(n)) if n >= 0 => n as u64,
            Some(other) => return Err(type_mismatch("Int", &other)),
            None => {
                return Err(RuntimeError::WrongArgumentCount {
                    expected: 1,
                    found: 0,
                    span: dummy_span(),
                });
            }
        };
        thread::sleep(std::time::Duration::from_millis(ms));
        self.stack.push(Value::new_async_ready(Value::new_unit()));
        Ok(())
    }

    fn intrinsic_block_on(&mut self, args: Vec<Value>) -> Result<(), RuntimeError> {
        // Host-side runner: drives an Async<T> or Handle<T> to completion
        // synchronously, pushing the inner T. Legal at the top level where
        // `await` is forbidden — the entry point from sync code into the
        // async runtime.
        let arg = args
            .into_iter()
            .next()
            .ok_or_else(|| RuntimeError::WrongArgumentCount {
                expected: 1,
                found: 0,
                span: dummy_span(),
            })?;
        let resolved = self.drive_to_value(arg)?;
        self.stack.push(resolved);
        Ok(())
    }

    fn intrinsic_shell_run_async(&mut self, args: Vec<Value>) -> Result<(), RuntimeError> {
        let cmd = match args.into_iter().next() {
            Some(Value::Str(s)) => s,
            Some(other) => return Err(type_mismatch("Str", &other)),
            None => {
                return Err(RuntimeError::WrongArgumentCount {
                    expected: 1,
                    found: 0,
                    span: dummy_span(),
                });
            }
        };
        let output = run_shell_command(&cmd)?;
        self.stack
            .push(Value::new_async_ready(Value::ShellOutput(output)));
        Ok(())
    }

    // ---------------- M9 algebraic-effects opcodes -----------------------

    /// Builds a `HandlerFrame` from the program's `handler_tables[idx]` and
    /// pushes it onto the handler stack. Records the current call_stack /
    /// value-stack / handler-stack depths so a later `Op::Perform` knows
    /// which slice of state to capture, and the post-handle IP so the
    /// no-resume path can fast-forward past `PopHandler`.
    fn op_push_handler(
        &mut self,
        table_idx: u16,
        body_end_offset: u16,
    ) -> Result<(), RuntimeError> {
        let table = self.handler_tables.get(table_idx as usize).ok_or_else(|| {
            RuntimeError::InvalidOperation {
                op: format!("PushHandler({table_idx}) out of range"),
                span: dummy_span(),
            }
        })?;

        // All entries in one `handle ... with { ... }` share one effect
        // (the parser/effects pass already enforces that all clauses
        // belong to the same effect). We still defensively pull the
        // effect tag from the first entry. If the table is empty, treat
        // it as a no-op handler (effect tag 0 with no ops); a perform
        // through it will fall through to enclosing handlers.
        let effect_tag = table.entries.first().map(|e| e.effect_tag).unwrap_or(0);
        let mut op_handlers: HashMap<u8, u16> = HashMap::with_capacity(table.entries.len());
        for entry in &table.entries {
            op_handlers.insert(entry.op_tag, entry.clause_chunk_idx);
        }

        // PushHandler always runs inside a call frame, so the call_stack
        // is non-empty here. The "handler-installing" frame is the
        // top-most frame at this moment; record its index so a later
        // Perform can capture it (and any frames above it that opened
        // during body evaluation).
        let frame_idx =
            self.call_stack
                .len()
                .checked_sub(1)
                .ok_or_else(|| RuntimeError::InvalidOperation {
                    op: "PushHandler outside any call frame".to_string(),
                    span: dummy_span(),
                })?;

        // The dispatch loop advanced the IP to "next instruction after
        // PushHandler" before invoking this opcode. Adding the compile-
        // time offset gives the IP after the matching `PopHandler`.
        let body_end_ip = self.call_stack[frame_idx].ip + body_end_offset as usize;

        let frame = HandlerFrame {
            effect_tag,
            op_handlers,
            frame_depth: frame_idx,
            stack_depth: self.stack.len(),
            handler_depth: self.handler_stack.len(),
            body_end_ip,
        };
        self.handler_stack.push(frame);
        Ok(())
    }

    /// Pops `argc` arguments off the value stack, walks the handler stack
    /// top-to-bottom for a frame matching `(effect_tag, op_tag)`, captures
    /// the suspended computation as a continuation, and transfers control
    /// to the matched clause chunk.
    ///
    /// If no handler matches, raises `RuntimeError::UnhandledEffect`.
    fn op_perform(&mut self, effect_tag: u16, op_tag: u8, argc: u8) -> Result<(), RuntimeError> {
        let argc = argc as usize;
        if self.stack.len() < argc {
            return Err(underflow());
        }

        // Pop args (declaration order — leftmost is at start of vec).
        let args_start = self.stack.len() - argc;
        let args: Vec<Value> = self.stack.drain(args_start..).collect();

        // Walk the handler stack top-to-bottom for a matching frame. If a
        // user-installed handler matches, prefer it over the built-in
        // semantics (lexical handlers always win — the built-in handler
        // is only the implicit fall-through when no user handler is in
        // scope).
        let matched_idx = self
            .handler_stack
            .iter()
            .rposition(|f| f.effect_tag == effect_tag && f.op_handlers.contains_key(&op_tag));

        let matched_idx = match matched_idx {
            Some(i) => i,
            None => {
                // No user handler — see if this is a built-in effect we
                // dispatch directly. The default `Async::Suspend` handler
                // drives the `Async<T>` operand to its inner value and
                // pushes the result, mirroring the M8 scheduler semantics.
                if self.builtin_effect_tags.async_suspend == Some((effect_tag, op_tag)) {
                    return self.builtin_async_suspend(args);
                }
                return Err(RuntimeError::UnhandledEffect {
                    effect_tag,
                    op_tag,
                    span: dummy_span(),
                });
            }
        };

        // Take the matching frame off the handler stack — while the
        // clause runs, this handler is *not* in scope (so the clause
        // can perform its own effects against the *outer* environment,
        // not against itself).
        let matched_frame = self.handler_stack.remove(matched_idx);
        let clause_chunk_idx = *matched_frame.op_handlers.get(&op_tag).unwrap();

        // Capture suspended state:
        //
        //   - operand-stack slice above `stack_depth`
        //   - call frames *above* the handler-installing frame
        //   - handler frames above `handler_depth`
        //   - the handler-installing frame's current IP (the instruction
        //     after `Op::Perform`), so resume can jump back here
        //
        // The handler-installing frame itself stays in `call_stack`. Its
        // IP is *fast-forwarded* to `body_end_ip`, so if the clause
        // returns without resuming, control naturally lands past
        // `PopHandler` with the clause's value as the handle expression's
        // result.
        let stack_above: Vec<Value> = self.stack.drain(matched_frame.stack_depth..).collect();
        let frames_above: Vec<CallFrame> = if self.call_stack.len() > matched_frame.frame_depth + 1
        {
            self.call_stack
                .drain(matched_frame.frame_depth + 1..)
                .collect()
        } else {
            Vec::new()
        };
        let handlers_above: Vec<HandlerFrame> =
            if self.handler_stack.len() > matched_frame.handler_depth {
                self.handler_stack
                    .drain(matched_frame.handler_depth..)
                    .collect()
            } else {
                Vec::new()
            };

        // Snapshot the handler-installing frame's IP, then bump it past
        // `PopHandler`.
        let handler_frame_ip = self.call_stack[matched_frame.frame_depth].ip;
        self.call_stack[matched_frame.frame_depth].ip = matched_frame.body_end_ip;

        let cont = Continuation {
            stack_above,
            frames_above,
            handlers_above,
            matched_frame,
            handler_frame_ip,
        };
        let cont_id = self.allocate_continuation(cont);

        // The clause chunk's leading slots are: argument values in
        // declaration order, followed by the continuation as the final
        // slot. (The compiler binds locals in this exact order — see
        // `compile_handler_clause`.)
        let mut clause_slots: Vec<Value> = args;
        clause_slots.push(Value::new_continuation(cont_id));

        self.call_stack.push(CallFrame {
            chunk_idx: clause_chunk_idx,
            ip: 0,
            slots: clause_slots,
        });
        Ok(())
    }

    /// Resumes a one-shot continuation. Pops `(cont_id, value)` from the
    /// stack (value on top — see `compile_resume`), restores the
    /// suspended state, pushes `value` as the result of the original
    /// `Op::Perform`, and continues execution there. Re-installs the
    /// handler frame so the resumed body can re-perform the same effect.
    fn op_resume(&mut self) -> Result<(), RuntimeError> {
        let value = self.pop()?;
        let cont_value = self.pop()?;
        let cont_id = match cont_value {
            Value::Continuation(id) => id,
            other => return Err(type_mismatch("Continuation", &other)),
        };

        let cont = self
            .continuations
            .get_mut(cont_id as usize)
            .and_then(|slot| slot.take())
            .ok_or(RuntimeError::ResumedTwice { span: dummy_span() })?;

        // The clause's call frame is currently on top of the call stack
        // (we are running inside it). Resuming transfers control back
        // into the suspended body — drop the clause frame so the
        // restored body state takes its place.
        self.call_stack.pop();

        // The handler-installing frame is now on top. Restore its IP
        // back to the perform site (`op_perform` set it to
        // `body_end_ip`).
        let frame_idx = cont.matched_frame.frame_depth;
        if let Some(installer) = self.call_stack.get_mut(frame_idx) {
            installer.ip = cont.handler_frame_ip;
        }

        // Restore in the same order the perform site captured:
        //   - reinstall the matched handler frame
        //   - reinstall any handlers that were nested above it
        //   - splice the captured call frames back on top
        //   - splice the operand-stack slice back
        //   - push the resume value as the result of the perform site
        self.handler_stack.push(cont.matched_frame);
        self.handler_stack.extend(cont.handlers_above);
        self.call_stack.extend(cont.frames_above);
        self.stack.extend(cont.stack_above);
        self.stack.push(value);

        Ok(())
    }

    /// Default handler for `perform Async::Suspend(future: <Async<T> | Handle<T> | T>)`.
    ///
    /// Installed implicitly around every program: when an `await` site (which
    /// the parser desugars to `perform Async::Suspend(future: expr)`) fires
    /// without a user-installed `Async` handler in scope, this method
    /// resolves the operand and pushes the result back onto the stack.
    ///
    /// Three operand shapes are accepted:
    ///   - `Value::Async(...)` — drive to its inner value (M8 semantics for
    ///     `Async` cells produced by `MakeAsync`, `sleep`, `shell_run_async`).
    ///   - `Value::Handle(...)` — block-join the worker thread.
    ///   - any other value — pass through unchanged. After the M9 parser
    ///     strips `Item::AsyncFn`, the compiler no longer emits `MakeAsync`
    ///     around plain async-fn calls, so the call returns the inner value
    ///     directly. `await` simply unwraps it.
    fn builtin_async_suspend(&mut self, args: Vec<Value>) -> Result<(), RuntimeError> {
        let future = args
            .into_iter()
            .next()
            .ok_or_else(|| RuntimeError::WrongArgumentCount {
                expected: 1,
                found: 0,
                span: dummy_span(),
            })?;
        let resolved = match future {
            Value::Async(_) | Value::Handle(_) => self.drive_to_value(future)?,
            other => other,
        };
        self.stack.push(resolved);
        Ok(())
    }

    /// Inserts a continuation into the slab and returns its index. Reuses
    /// `None` slots to keep ids dense; slots are otherwise append-only.
    fn allocate_continuation(&mut self, cont: Continuation) -> u32 {
        for (i, slot) in self.continuations.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(cont);
                return i as u32;
            }
        }
        let id = self.continuations.len() as u32;
        self.continuations.push(Some(cont));
        id
    }
}

/// Shells out to `/bin/sh -c <cmd>` and returns the captured `ShellOutput`.
/// Used by `shell_run_async`; mirrors the synchronous shell runner that
/// `ferric_stdlib::__shell_exec` provides for `$ ...` outside async.
fn run_shell_command(cmd: &str) -> Result<ShellOutput, RuntimeError> {
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .map_err(|e| RuntimeError::NativeFunctionError {
            message: format!("shell_run_async: {e}"),
            span: dummy_span(),
        })?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let exit_code = out.status.code().unwrap_or(-1);
    Ok(ShellOutput { stdout, exit_code })
}

fn constant_to_value(c: &Constant) -> Value {
    match c {
        Constant::Int(n) => Value::new_int(*n),
        Constant::Float(f) => Value::new_float(*f),
        Constant::Str(s) => Value::new_str(s.clone()),
        Constant::Bool(b) => Value::new_bool(*b),
        Constant::Fn(idx) => Value::new_fn(*idx),
        Constant::NativeFn(sym) => Value::new_native_fn(*sym),
    }
}

fn value_to_native(v: &Value) -> NativeValue {
    match v {
        Value::Int(n) => NativeValue::Int(*n),
        Value::Float(f) => NativeValue::Float(*f),
        Value::Bool(b) => NativeValue::Bool(*b),
        Value::Str(s) => NativeValue::Str(s.clone()),
        Value::Unit => NativeValue::Unit,
        Value::ShellOutput(out) => NativeValue::ShellOutput(out.clone()),
        Value::Array(elems) => NativeValue::Array(elems.iter().map(value_to_native).collect()),
        // Functions, structs, enums, and closures don't cross the native
        // boundary; surface them as Unit so a wrong-type native call produces
        // a descriptive error inside the native rather than a panic here.
        Value::Fn(_)
        | Value::NativeFn(_)
        | Value::Struct(_)
        | Value::Variant(_, _)
        | Value::Tuple(_)
        | Value::Closure { .. } => NativeValue::Unit,
        // Async / Handle never cross the regular native boundary: spawn,
        // join, sleep, and shell_run_async are dispatched as VM intrinsics
        // (with access to scheduler state) rather than via NativeRegistry.
        // Surface them as Unit if a regular native ever sees one — that
        // would indicate a stdlib registration mistake.
        Value::Async(_) | Value::Handle(_) => NativeValue::Unit,
        // Continuations are opaque VM-only values; they should never
        // cross the native boundary.
        Value::Continuation(_) => NativeValue::Unit,
    }
}

fn native_to_value(v: NativeValue) -> Value {
    match v {
        NativeValue::Int(n) => Value::new_int(n),
        NativeValue::Float(f) => Value::new_float(f),
        NativeValue::Bool(b) => Value::new_bool(b),
        NativeValue::Str(s) => Value::new_str(s),
        NativeValue::Unit => Value::new_unit(),
        NativeValue::ShellOutput(out) => Value::ShellOutput(out),
        NativeValue::Array(elems) | NativeValue::List(elems) | NativeValue::Tuple(elems) => {
            Value::new_array(elems.into_iter().map(native_to_value).collect())
        }
        // Extended stdlib variants land in the VM as opaque Unit for now —
        // a future task lifts them into proper Value variants. Until then the
        // values are still observable via the stdlib's native registry path.
        NativeValue::Bytes(_)
        | NativeValue::BytesMut(_)
        | NativeValue::ListMut(_)
        | NativeValue::Map(_)
        | NativeValue::MapMut(_)
        | NativeValue::Set(_)
        | NativeValue::Option(_)
        | NativeValue::Result(_)
        | NativeValue::Tuple2(_, _)
        | NativeValue::Json(_)
        | NativeValue::Time(_)
        | NativeValue::Duration(_)
        | NativeValue::Regex(_)
        | NativeValue::Match(_)
        | NativeValue::Ordering(_)
        | NativeValue::Rng(_)
        | NativeValue::Closure(_)
        | NativeValue::HttpResponse(_)
        | NativeValue::HttpRequestBuilder(_)
        | NativeValue::ServeServer(_)
        | NativeValue::ServeRequest(_)
        | NativeValue::ServeResponse(_)
        | NativeValue::TcpStream(_)
        | NativeValue::TcpListener(_)
        | NativeValue::UdpSocket(_)
        | NativeValue::FileWriter(_)
        | NativeValue::ProcOutput(_)
        | NativeValue::ProcCommand(_)
        | NativeValue::ProcProcess(_)
        | NativeValue::Logger(_)
        | NativeValue::LogLevel(_)
        | NativeValue::SyncSender(_)
        | NativeValue::SyncReceiver(_)
        | NativeValue::SyncMutex(_)
        | NativeValue::SyncOnce(_) => Value::new_unit(),
    }
}

/// Adds a post-advance jump offset to an `ip`.
fn apply_offset(ip: usize, off: i16) -> usize {
    (ip as i64 + off as i64) as usize
}

fn dummy_span() -> Span {
    Span::new(0, 0)
}

fn underflow() -> RuntimeError {
    RuntimeError::StackUnderflow { span: dummy_span() }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "Int",
        Value::Float(_) => "Float",
        Value::Bool(_) => "Bool",
        Value::Str(_) => "Str",
        Value::Unit => "Unit",
        Value::Fn(_) => "Fn",
        Value::NativeFn(_) => "NativeFn",
        Value::ShellOutput(_) => "ShellOutput",
        Value::Struct(_) => "Struct",
        Value::Variant(_, _) => "Variant",
        Value::Tuple(_) => "Tuple",
        Value::Array(_) => "Array",
        Value::Closure { .. } => "Closure",
        Value::Async(_) => "Async",
        Value::Handle(_) => "Handle",
        Value::Continuation(_) => "Continuation",
    }
}

fn type_mismatch(expected: &str, found: &Value) -> RuntimeError {
    RuntimeError::TypeMismatch {
        expected: expected.to_string(),
        found: type_name(found).to_string(),
        span: dummy_span(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_common::{Interner, Symbol};
    use ferric_infer::typecheck;
    use ferric_lexer::lex;
    use ferric_parser::parse;
    use ferric_resolve::resolve_with_natives;
    use ferric_stdlib::register_stdlib;

    fn run_source(src: &str) -> Result<Value, RuntimeError> {
        let mut interner = Interner::new();
        let mut natives = NativeRegistry::new();
        register_stdlib(&mut natives, &mut interner);

        let native_fns: Vec<(Symbol, Vec<Symbol>)> = vec![
            (interner.intern("println"), vec![interner.intern("s")]),
            (interner.intern("print"), vec![interner.intern("s")]),
            (interner.intern("int_to_str"), vec![interner.intern("n")]),
            (interner.intern("float_to_str"), vec![interner.intern("n")]),
            (interner.intern("bool_to_str"), vec![interner.intern("b")]),
            (interner.intern("int_to_float"), vec![interner.intern("n")]),
            (
                interner.intern("shell_stdout"),
                vec![interner.intern("output")],
            ),
            (
                interner.intern("shell_exit_code"),
                vec![interner.intern("output")],
            ),
        ];

        let lex_result = lex(src, &mut interner);
        assert!(lex_result.errors.is_empty(), "lex: {:?}", lex_result.errors);
        let parse_result = parse(&lex_result);
        assert!(
            parse_result.errors.is_empty(),
            "parse: {:?}",
            parse_result.errors
        );
        let resolve_result = resolve_with_natives(&parse_result, &native_fns);
        assert!(
            resolve_result.errors.is_empty(),
            "resolve: {:?}",
            resolve_result.errors
        );
        let registry = ferric_common::TraitRegistry::new();
        let type_result = typecheck(&parse_result, &resolve_result, &interner, &registry);
        assert!(
            type_result.errors.is_empty(),
            "types: {:?}",
            type_result.errors
        );
        let program =
            ferric_compiler::compile(&parse_result, &resolve_result, &type_result, &interner);

        let mut vm = BytecodeVM::new();
        vm.run(program, natives, &interner)
    }

    #[test]
    fn integer_add() {
        assert_eq!(run_source("1 + 2").unwrap(), Value::Int(3));
    }

    #[test]
    fn integer_precedence() {
        assert_eq!(run_source("1 + 2 * 3").unwrap(), Value::Int(7));
    }

    #[test]
    fn let_then_use() {
        assert_eq!(run_source("let x: Int = 5\nx").unwrap(), Value::Int(5));
    }

    #[test]
    fn if_true_branch() {
        assert_eq!(
            run_source("if true { 1 } else { 2 }").unwrap(),
            Value::Int(1)
        );
    }

    #[test]
    fn if_false_branch() {
        assert_eq!(
            run_source("if false { 1 } else { 2 }").unwrap(),
            Value::Int(2)
        );
    }

    #[test]
    fn while_counts_to_five() {
        // Sum 0..5 → 0+1+2+3+4 = 10.
        let src = "\
let mut i: Int = 0
let mut sum: Int = 0
while i < 5 {
    sum = sum + i
    i = i + 1
}
sum
";
        assert_eq!(run_source(src).unwrap(), Value::Int(10));
    }

    #[test]
    fn break_exits_loop() {
        let src = "\
let mut i: Int = 0
loop {
    if i == 3 { break }
    i = i + 1
}
i
";
        assert_eq!(run_source(src).unwrap(), Value::Int(3));
    }

    #[test]
    fn recursive_function() {
        // Straight recursion is a good exercise for frame push/pop.
        let src = "\
fn fib(n: Int) -> Int {
    if n <= 1 { n } else { fib(n: n - 1) + fib(n: n - 2) }
}
fib(n: 10)
";
        assert_eq!(run_source(src).unwrap(), Value::Int(55));
    }

    #[test]
    fn string_concat() {
        let src = r#""hello " + "world""#;
        assert_eq!(
            run_source(src).unwrap(),
            Value::Str("hello world".to_string())
        );
    }

    #[test]
    fn division_by_zero_reports() {
        let err = run_source("5 / 0").unwrap_err();
        assert!(matches!(err, RuntimeError::DivisionByZero { .. }));
    }

    #[test]
    fn native_println_does_not_crash() {
        // println returns Unit.
        let src = r#"println(s: "hi")"#;
        assert_eq!(run_source(src).unwrap(), Value::Unit);
    }

    #[test]
    fn require_passes_silently() {
        let src = "\
let x: Int = 5
require x > 0
x
";
        assert_eq!(run_source(src).unwrap(), Value::Int(5));
    }

    #[test]
    fn require_fails_with_message() {
        let src = "\
let x: Int = -1
require x > 0, \"x must be positive\"
";
        let err = run_source(src).unwrap_err();
        match err {
            RuntimeError::RequireError { message, .. } => {
                assert_eq!(message.as_deref(), Some("x must be positive"));
            }
            other => panic!("expected RequireError, got {other:?}"),
        }
    }

    // ============================================================
    // M4 — structs / enums / tuples / patterns
    // ============================================================

    #[test]
    fn struct_literal_and_field_access() {
        let src = "\
struct Point { x: Int, y: Int }
let p = Point { x: 7, y: 9 }
p.x + p.y
";
        assert_eq!(run_source(src).unwrap(), Value::Int(16));
    }

    #[test]
    fn enum_match_picks_correct_arm() {
        let src = "\
enum Shape { Circle(Int), Rectangle(Int, Int) }
fn area(s: Shape) -> Int {
    match s {
        Shape::Circle(r) => r * r * 3,
        Shape::Rectangle(w, h) => w * h,
    }
}
area(s: Shape::Rectangle(3, 4))
";
        assert_eq!(run_source(src).unwrap(), Value::Int(12));
    }

    #[test]
    fn match_wildcard_covers_remaining_variants() {
        let src = "\
enum Color { Red, Green, Blue }
fn name(c: Color) -> Str {
    match c {
        Color::Red => \"red\",
        _ => \"other\",
    }
}
name(c: Color::Blue)
";
        assert_eq!(run_source(src).unwrap(), Value::Str("other".to_string()));
    }

    #[test]
    fn tuple_pattern_destructures() {
        let src = "\
let t = (10, 20)
match t {
    (a, b) => a + b,
}
";
        assert_eq!(run_source(src).unwrap(), Value::Int(30));
    }

    #[test]
    fn struct_pattern_with_literal_subpattern() {
        let src = "\
struct Pt { x: Int, y: Int }
fn classify(p: Pt) -> Str {
    match p {
        Pt { x: 0, y: 0 } => \"origin\",
        Pt { x, y: 0 } => \"x-axis\",
        Pt { x: 0, y } => \"y-axis\",
        Pt { x, y } => \"plane\",
    }
}
classify(p: Pt { x: 3, y: 0 })
";
        assert_eq!(run_source(src).unwrap(), Value::Str("x-axis".to_string()));
    }

    #[test]
    fn require_set_recovers() {
        // set_fn's `x = 5` must mutate the outer mutable slot.
        let src = "\
let mut x: Int = -1
require x > 0, \"x must be positive\", set: || { x = 5 }
x
";
        assert_eq!(run_source(src).unwrap(), Value::Int(5));
    }
}
