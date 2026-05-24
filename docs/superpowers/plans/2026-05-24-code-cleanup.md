# Code Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove commented-out code and dead code paths from Rust and TypeScript source files using a hybrid tool + manual approach.

**Architecture:** Three-phase cleanup: (1) automated tool analysis with clippy/udeps/tsc, (2) manual review of recently modified desktop daemon files, (3) pattern-based removal of obvious commented code across the codebase. Each phase followed by verification.

**Tech Stack:** Rust (cargo clippy, cargo-udeps), TypeScript (tsc), ripgrep

---

## Task 1: Phase 1 - Automated Rust Analysis

**Files:**
- Analyze: All Rust files in `crates/`
- Output: Terminal report

- [ ] **Step 1: Run cargo clippy with dead code warnings**

```bash
cargo clippy --all-targets --all-features -- -W dead_code -W unused_imports -W unused_variables -W unreachable_code 2>&1 | tee /tmp/clippy-report.txt
```

Expected: List of warnings with file paths and line numbers

- [ ] **Step 2: Check if cargo-udeps is installed**

```bash
cargo install cargo-udeps --locked || echo "cargo-udeps already installed"
```

Expected: Installation confirmation or already installed message

- [ ] **Step 3: Run cargo-udeps to find unused dependencies**

```bash
cargo +nightly udeps --all-targets 2>&1 | tee /tmp/udeps-report.txt
```

Expected: List of unused dependencies (if any)

- [ ] **Step 4: Review clippy report and create cleanup list**

```bash
cat /tmp/clippy-report.txt | grep -E "(unused|dead_code|unreachable)" | head -50
```

Expected: Filtered list of actionable items

Note: Save this output for reference in subsequent tasks

---

## Task 2: Phase 1 - Automated TypeScript Analysis

**Files:**
- Analyze: `crates/iota-desktop/src/**/*.ts`, `crates/iota-desktop/src/**/*.tsx`
- Output: Terminal report

- [ ] **Step 1: Run TypeScript compiler with unused checks**

```bash
cd crates/iota-desktop && npx tsc --noUnusedLocals --noUnusedParameters --noEmit 2>&1 | tee /tmp/tsc-report.txt
```

Expected: List of unused variables/parameters with file paths and line numbers

- [ ] **Step 2: Review TypeScript report**

```bash
cat /tmp/tsc-report.txt | grep -E "is declared but" | head -30
```

Expected: Filtered list of unused declarations

Note: Save this output for reference in subsequent tasks

---

## Task 3: Phase 2 - Clean desktop.rs

**Files:**
- Modify: `crates/iota-core/src/daemon/desktop.rs`

- [ ] **Step 1: Read the file and identify cleanup targets**

Read `crates/iota-core/src/daemon/desktop.rs` and look for:
- Commented-out code blocks (consecutive `//` lines)
- Debug print statements (`println!`, `eprintln!`, `dbg!`)
- Unused imports from clippy report
- Unreachable code after returns

- [ ] **Step 2: Remove unused imports flagged by clippy**

Check clippy report for unused imports in this file and remove them.

- [ ] **Step 3: Remove debug print statements**

Search for and remove any `println!`, `eprintln!`, or `dbg!` calls that are not in test code.

- [ ] **Step 4: Remove commented-out code blocks**

Remove any consecutive comment lines (3+ lines) that contain code patterns (`{`, `}`, `;`, `fn`, `let`).

- [ ] **Step 5: Verify the file compiles**

```bash
cargo check -p iota-core
```

Expected: No compilation errors

- [ ] **Step 6: Run tests for this module**

```bash
cargo test -p iota-core daemon::desktop
```

Expected: All tests pass

- [ ] **Step 7: Commit changes**

```bash
git add crates/iota-core/src/daemon/desktop.rs
git commit -m "refactor: clean up desktop.rs - remove dead code and debug statements"
```

---

## Task 4: Phase 2 - Clean desktop_tests.rs

**Files:**
- Modify: `crates/iota-core/src/daemon/desktop_tests.rs`

- [ ] **Step 1: Read the file and identify cleanup targets**

Read `crates/iota-core/src/daemon/desktop_tests.rs` and look for:
- Commented-out test cases
- Unused imports from clippy report
- Unused helper functions

- [ ] **Step 2: Remove unused imports**

Check clippy report and remove unused imports.

- [ ] **Step 3: Remove commented-out test code**

Remove any commented-out test functions or assertions.

- [ ] **Step 4: Verify tests compile and run**

```bash
cargo test -p iota-core daemon::desktop_tests
```

Expected: All tests pass

- [ ] **Step 5: Commit changes**

```bash
git add crates/iota-core/src/daemon/desktop_tests.rs
git commit -m "refactor: clean up desktop_tests.rs - remove unused code"
```

---

## Task 5: Phase 2 - Clean Tauri lib.rs

**Files:**
- Modify: `crates/iota-desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Read the file and identify cleanup targets**

Read `crates/iota-desktop/src-tauri/src/lib.rs` and look for:
- Commented-out code blocks
- Debug print statements
- Unused imports from clippy report
- Unreachable code

- [ ] **Step 2: Remove unused imports**

Check clippy report and remove unused imports.

- [ ] **Step 3: Remove debug statements**

Remove any `println!`, `eprintln!`, or `dbg!` calls not in test code.

- [ ] **Step 4: Remove commented-out code**

Remove commented-out code blocks.

- [ ] **Step 5: Verify compilation**

```bash
cargo check -p iota-desktop
```

Expected: No compilation errors

- [ ] **Step 6: Commit changes**

```bash
git add crates/iota-desktop/src-tauri/src/lib.rs
git commit -m "refactor: clean up tauri lib.rs - remove dead code"
```

---

## Task 6: Phase 2 - Clean TypeScript api.ts

**Files:**
- Modify: `crates/iota-desktop/src/api.ts`

- [ ] **Step 1: Read the file and identify cleanup targets**

Read `crates/iota-desktop/src/api.ts` and look for:
- Commented-out code blocks
- Unused imports from tsc report
- Debug console.log statements
- Unused functions/variables

- [ ] **Step 2: Remove unused imports**

Check tsc report and remove unused imports.

- [ ] **Step 3: Remove debug console statements**

Remove any `console.log`, `console.debug` calls used for debugging.

- [ ] **Step 4: Remove commented-out code**

Remove commented-out code blocks.

- [ ] **Step 5: Verify TypeScript compilation**

```bash
cd crates/iota-desktop && npx tsc --noEmit
```

Expected: No type errors

- [ ] **Step 6: Commit changes**

```bash
git add crates/iota-desktop/src/api.ts
git commit -m "refactor: clean up api.ts - remove dead code and debug logs"
```

---

## Task 7: Phase 2 - Clean React Components

**Files:**
- Modify: `crates/iota-desktop/src/components/ChatWorkbench.tsx`
- Modify: `crates/iota-desktop/src/components/ConfigPanel.tsx`
- Modify: `crates/iota-desktop/src/components/RightInspector.tsx`

- [ ] **Step 1: Clean ChatWorkbench.tsx**

Read the file, remove:
- Unused imports from tsc report
- Debug console statements
- Commented-out JSX/code blocks

- [ ] **Step 2: Clean ConfigPanel.tsx**

Read the file, remove:
- Unused imports
- Debug console statements
- Commented-out code

- [ ] **Step 3: Clean RightInspector.tsx**

Read the file, remove:
- Unused imports
- Debug console statements
- Commented-out code

- [ ] **Step 4: Verify TypeScript compilation**

```bash
cd crates/iota-desktop && npx tsc --noEmit
```

Expected: No type errors

- [ ] **Step 5: Commit changes**

```bash
git add crates/iota-desktop/src/components/
git commit -m "refactor: clean up React components - remove dead code and debug logs"
```

---

## Task 8: Phase 2 - Clean TypeScript Support Files

**Files:**
- Modify: `crates/iota-desktop/src/types.ts`
- Modify: `crates/iota-desktop/src/turnReducer.ts`

- [ ] **Step 1: Clean types.ts**

Read the file, remove:
- Unused type definitions from tsc report
- Commented-out types
- Unused imports

- [ ] **Step 2: Clean turnReducer.ts**

Read the file, remove:
- Unused imports
- Debug console statements
- Commented-out code

- [ ] **Step 3: Verify TypeScript compilation**

```bash
cd crates/iota-desktop && npx tsc --noEmit
```

Expected: No type errors

- [ ] **Step 4: Run reducer tests**

```bash
cd crates/iota-desktop && npm test turnReducer.test.ts
```

Expected: All tests pass

- [ ] **Step 5: Commit changes**

```bash
git add crates/iota-desktop/src/types.ts crates/iota-desktop/src/turnReducer.ts
git commit -m "refactor: clean up types and reducer - remove unused definitions"
```

---

## Task 9: Phase 3 - Pattern-Based Cleanup (Rust)

**Files:**
- Analyze: All `*.rs` files in `crates/`

- [ ] **Step 1: Find commented-out imports**

```bash
rg "^\s*// use " --type rust crates/ | tee /tmp/commented-imports-rust.txt
```

Expected: List of commented-out use statements

- [ ] **Step 2: Find commented-out function signatures**

```bash
rg "^\s*// (pub )?fn " --type rust crates/ | tee /tmp/commented-functions-rust.txt
```

Expected: List of commented-out function definitions

- [ ] **Step 3: Find dbg! macros in non-test code**

```bash
rg "dbg!\(" --type rust crates/ | grep -v "#\[cfg(test)\]" | grep -v "tests.rs" | tee /tmp/dbg-macros.txt
```

Expected: List of dbg! calls in production code

- [ ] **Step 4: Review and remove obvious cases**

For each file in the reports, read the file and remove the commented-out code if it's clearly dead code (not documentation or intentional examples).

- [ ] **Step 5: Verify compilation after removals**

```bash
cargo check --all-targets
```

Expected: No compilation errors

- [ ] **Step 6: Commit pattern-based cleanup**

```bash
git add -u
git commit -m "refactor: remove commented-out code patterns across Rust codebase"
```

---

## Task 10: Phase 3 - Pattern-Based Cleanup (TypeScript)

**Files:**
- Analyze: All `*.ts`, `*.tsx` files in `crates/iota-desktop/src/`

- [ ] **Step 1: Find commented-out imports**

```bash
rg "^\s*// import " --type ts --type tsx crates/iota-desktop/src/ | tee /tmp/commented-imports-ts.txt
```

Expected: List of commented-out import statements

- [ ] **Step 2: Find commented-out function definitions**

```bash
rg "^\s*// (export )?(function|const|let) " --type ts --type tsx crates/iota-desktop/src/ | tee /tmp/commented-functions-ts.txt
```

Expected: List of commented-out function/const definitions

- [ ] **Step 3: Review and remove obvious cases**

For each file in the reports, read the file and remove the commented-out code if it's clearly dead code.

- [ ] **Step 4: Verify TypeScript compilation**

```bash
cd crates/iota-desktop && npx tsc --noEmit
```

Expected: No type errors

- [ ] **Step 5: Commit pattern-based cleanup**

```bash
git add -u
git commit -m "refactor: remove commented-out code patterns across TypeScript codebase"
```

---

## Task 11: Phase 4 - Final Verification

**Files:**
- Verify: All modified files

- [ ] **Step 1: Full build check**

```bash
cargo build --all-features
```

Expected: Successful build with no warnings

- [ ] **Step 2: Run full test suite**

```bash
cargo test
```

Expected: All 367 tests pass

- [ ] **Step 3: Run clippy with warnings as errors**

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: No warnings

- [ ] **Step 4: Verify TypeScript compilation**

```bash
cd crates/iota-desktop && npx tsc --noEmit
```

Expected: No type errors

- [ ] **Step 5: Manual smoke test - TUI**

```bash
cargo run -p iota-cli --quiet
```

Test:
1. TUI launches successfully
2. Can send a prompt
3. Can switch backends with Ctrl+B
4. Can exit with Ctrl+C twice

Expected: All basic flows work

- [ ] **Step 6: Manual smoke test - Desktop**

```bash
cd crates/iota-desktop && npm run tauri dev
```

Test:
1. Desktop app launches
2. Can send a message
3. Can view turn details in inspector
4. Backend status shows correctly

Expected: All basic flows work

- [ ] **Step 7: Create final summary commit if needed**

If there were any final adjustments during verification:

```bash
git add -u
git commit -m "refactor: final cleanup adjustments after verification"
```

- [ ] **Step 8: Review git log**

```bash
git log --oneline -10
```

Expected: Clean series of cleanup commits

---

## Success Criteria

- ✅ All compiler warnings for dead code, unused imports, and unused variables resolved
- ✅ No commented-out code blocks in priority zone files (desktop daemon integration)
- ✅ Test suite passes with all 367 tests
- ✅ Codebase builds cleanly with no warnings
- ✅ Manual smoke tests pass for both TUI and desktop
- ✅ All changes committed with clear messages

## Rollback Plan

If issues arise during or after cleanup:

```bash
# Revert all cleanup commits
git log --oneline | grep "refactor: clean" | cut -d' ' -f1 | xargs -I{} git revert {}

# Or reset to before cleanup started
git reset --hard <commit-before-cleanup>
```
