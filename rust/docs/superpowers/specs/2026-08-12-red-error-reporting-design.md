# Red Error Reporting for Terminal-Crash Behaviors — Design

> Date: 2026-08-12
> Status: Approved by user

## Goal

Convert every behavior in `crates/claw-cli/src/main.rs` that currently crashes the terminal (raw panics, abrupt `process::exit` without a visible error, plain-text top-level errors) into a **red error report**. Red ANSI coloring is applied only when stderr is a TTY, so piped/redirected output stays plain and JSON output stays machine-parseable.

## Scope

All crash behaviors in `crates/claw-cli/src/main.rs`:

1. **Top-level error handler** (`main()`, lines 229-274) — plain `error: {message}`.
2. **`process::exit` paths** — resume-session failure (2992), resume-command failures (3045/3064/3079/3116), broad-cwd abort/non-interactive (3857/3883).
3. **Production panic/expect points** — `BuiltRuntime` invariants (4101/4134/4142), `print_status` (5333), `print_sandbox_status` (5390), prompt-progress poison expects (7644-7739), `message_cache.unwrap()` (8276), `image_cache.lock().unwrap()` (8647), infallible `write!` to String (9447/9453).

Out of scope (explicitly not changed):
- Exit-code *semantics* for the normal/resume paths — `exit(1)`/`exit(2)` are kept as-is, only text becomes red. The only new exit code is 70 for internal invariants.
- Installing a global `panic::set_hook`.
- Changing `run()`'s error type/propagation structure.
- Test-only panics (lines 10519+).

## Architecture

### New helper functions (top level of `main.rs`)

```rust
/// Distinct exit code for uncaught/unexpected internal failures
/// (sysexits EX_SOFTWARE = 70). Signals a program bug, not a user mistake,
/// and is distinct from the normal error exit(1) and usage exit(2).
const EXIT_INTERNAL_ERROR: i32 = 70;

/// Render a message in red, but only when stderr is a terminal. Piped or
/// redirected output stays plain so ANSI escapes never pollute logs or
/// machine-parsed streams.
fn render_error_red(message: &str) -> String {
    if io::stderr().is_terminal() {
        format!("\x1b[31m{message}\x1b[0m")
    } else {
        message.to_string()
    }
}

/// Print a red error line to stderr (respects TTY detection).
fn eprint_red_error(message: &str) {
    eprintln!("{}", render_error_red(message));
}

/// Internal invariant violated: print a red "internal error" line and exit
/// with EXIT_INTERNAL_ERROR. Used at unreachable-invariant panic sites where
/// continuing would be wrong but a Rust panic (raw thread message + backtrace
/// noise) is not the desired terminal experience.
fn internal_error(message: &str) -> ! {
    eprint_red_error(&format!("internal error: {message}"));
    std::process::exit(EXIT_INTERNAL_ERROR);
}
```

`io::IsTerminal` is already imported (main.rs line 21 `use io::{self, IsTerminal, ...}`).

## Crash-Site Conversion

### Top-level `main()` error handler (229-274)

- Text branch: wrap the `error: {message}` line via `render_error_red`. The `[error-kind: {kind}]` prefix stays plain text (machine-scanning convention) — only the `error:` line is colored. Actually: color the whole `error: {message}` line including the prefix on that line; the separate `[error-kind]` line remains plain to preserve existing stderr scanning.

  Concretely, the current text branch:

  ```rust
  eprintln!(
      "[error-kind: {kind}]\nerror: {message}\n\nRun `claw --help` for usage."
  );
  ```

  becomes:

  ```rust
  eprintln!(
      "[error-kind: {kind}]\n{}\n\nRun `claw --help` for usage.",
      render_error_red(&format!("error: {message}"))
  );
  ```

- JSON branch: unchanged (must stay valid JSON, never ANSI).
- `std::process::exit(1)` unchanged.

### `process::exit` paths

Each site prints a plain error then exits. Wrap the printed text with `eprint_red_error(...)` (text branches only; JSON branches unchanged).

| Site | Text branch today | Becomes |
|---|---|---|
| 2992 `failed to restore session: {error}` | `eprintln!("failed to restore session: {error}")` | `eprint_red_error(&format!("failed to restore session: {error}"))` |
| 3043 `/{cmd_root} is not yet implemented...` | `eprintln!(...)` | `eprint_red_error(...)` |
| 3062 `unsupported resumed command: {raw_command}` | `eprintln!(...)` | `eprint_red_error(...)` |
| 3077 `{error}` (resume parse) | `eprintln!("{error}")` | `eprint_red_error(&error.to_string())` |
| 3115 `{error}` (resume run) | `eprintln!("{error}")` | `eprint_red_error(&error.to_string())` |
| 3856 `Aborted.` | `eprintln!("Aborted.")` | `eprint_red_error("Aborted.")` (then `exit(0)`) |
| 3880 `error: {message}` (broad-cwd non-interactive) | `eprintln!("error: {message}")` | `eprint_red_error(&format!("error: {message}"))` (then `exit(1)`) |

All exit codes stay as they are today.

### `BuiltRuntime` invariant expects (4101, 4134, 4142)

Replace `expect("...")` with `unwrap_or_else(|_| internal_error("runtime unavailable while ..."))`:

- 4101: `internal_error("runtime should exist before installing hook abort signal")`
- 4134: `internal_error("runtime should exist while built runtime is alive")`
- 4142: `internal_error("runtime should exist while built runtime is alive")`

### REPL-internal crash points — MUST NOT exit the session (5333, 5390)

These two are dispatched from inside the REPL loop (`handle_repl_command` → `print_status`/`print_sandbox_status`). Calling `process::exit` or panicking here kills the entire interactive session — the exact crash behavior this task removes. **They must propagate as errors, not exit.**

- `print_status(&self)` (5318) currently `-> ()`, calls `status_context(Some(&self.session.path)).expect("status context should load")` at 5333.
  Change signature to `-> Result<(), Box<dyn std::error::Error>>` and replace the `expect` with `?`.
- `print_sandbox_status()` (5389) currently `-> ()`, calls `env::current_dir().expect("current dir")` at 5390.
  Change signature to `-> Result<(), Box<dyn std::error::Error>>` and replace the `expect` with `?`.
- Update dispatch sites in `handle_repl_command` (5125-5131):
  ```rust
  SlashCommand::Status => {
      self.print_status()?;
      false
  }
  SlashCommand::Sandbox => {
      Self::print_sandbox_status()?;
      false
  }
  ```

The errors bubble through the existing `handle_repl_command` → `run_repl` → `run()` → `main()` chain, which renders them red and exits(1) — the REPL terminates on the failure, but with a clean red message instead of a raw panic that would leave the terminal in raw mode. (REPL-error propagation is the existing design; we only convert the panic sites into propagatable errors.)

### Poison-recoverable expects (7644-7739, 8647)

These are Mutex-guard sites where a panic while holding the lock would poison it. Recover instead of crashing (matches the pattern used in the `agents` crate):

- 7644/7669/7694/7726: `.expect("internal prompt progress state poisoned")` → `.unwrap_or_else(std::sync::PoisonError::into_inner)`
- 7739: `.expect("internal prompt progress output lock poisoned")` → `.unwrap_or_else(std::sync::PoisonError::into_inner)`
- 8647: `arc.lock().unwrap()` → `arc.lock().unwrap_or_else(std::sync::PoisonError::into_inner)`

### Structural unwrap (8276)

The `message_cache.as_mut().unwrap()` is guarded by an outer `if let Some(cache) = &self.message_cache` — the unwrap is redundant. Restructure the inner block to avoid it:

```rust
if let Some(cache) = self.message_cache.as_mut() {
    if cache.last_ptr == msg_ptr && cache.last_len <= msg_len {
        // ... existing body ...
    }
}
```

This requires reworking the borrow: the outer match currently borrows `&self.message_cache` immutably then re-borrows mutably. Restructure so the mutable borrow is taken once (see implementation plan for the exact shape).

### Infallible `write!` to String (9447, 9453)

`write!(to_string)` cannot fail; the `expect("write to string")` is noise. Replace with `let _ = write!(...)`.

## TTY / NO_COLOR Behavior

- Red only when `io::stderr().is_terminal()`.
- No `NO_COLOR` handling (out of scope — the existing codebase does not honor it).
- JSON output paths are never colorized.

## Testing

- **Pure helper test**: extract the color decision into `fn should_color(flag: bool) -> bool` (or test `render_error_red` indirectly). Simplest: unit-test `render_error_red` is not directly TTY-testable; add a helper `fn apply_red_if(message: &str, red: bool) -> String` that `render_error_red` delegates to, and unit-test `apply_red_if(true/false)`.
- **Existing test safety**: the `[error-kind: {kind}]` prefix and all text messages remain intact — red is added as ANSI wrappers only, and the test environment (non-TTY stderr) gets zero ANSI. Run the full claw-cli test suite to confirm no regression.
- **Manual verification**: run `claw` with a failing command in a terminal (red) and with `| cat` (plain).

## Trade-offs / Decisions

1. **REPL-internal sites propagate instead of exiting** — required to actually remove the session-killing behavior; the naive "red + exit" would have kept killing the REPL.
2. **Internal invariants exit(70) with `internal error:` prefix** — the prefix distinguishes a program bug from a user error; 70 is the conventional EX_SOFTWARE.
3. **Poison sites recover, not exit** — recovery is semantically correct (the guard data is still valid), consistent with the `agents` crate.
4. **`[error-kind]` line stays plain** — it is a machine-scanning convention; only the human-facing `error:` line is colored.
5. **No global panic hook** — per-site handling keeps behavior explicit and testable; a hook would only catch panics we already convert.

## Non-Goals

- Change exit-code semantics of normal/resume paths.
- Honor `NO_COLOR`.
- Refactor `run()`/`run_repl` error types.
- Touch test-only code.
