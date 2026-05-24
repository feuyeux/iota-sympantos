# Code Cleanup Design

**Date:** 2026-05-24  
**Objective:** Remove commented-out code and dead code paths from Rust and TypeScript source files

## Scope

Clean up unused code across the iota-sympantos codebase (~2000 source files) using a hybrid approach that balances safety and thoroughness.

**In scope:**
- Commented-out code blocks (multi-line comments, consecutive single-line comments)
- Dead code paths (unused functions, unreachable branches)
- Unused imports and variables
- Debug print statements in production code
- Unused dependencies

**Out of scope:**
- Documentation comments and explanatory notes
- TODO/FIXME comments with context
- Intentionally disabled code with explanations
- Build artifacts (target/, node_modules/, dist/)

## Approach: Hybrid Manual + Tool

### Phase 1: Automated Tool Analysis

Use compiler-backed tools to identify genuinely unused code:

**Rust analysis:**
```bash
cargo clippy -- -W dead_code -W unused_imports -W unused_variables -W unreachable_code
cargo +nightly udeps  # unused dependencies
```

**TypeScript analysis:**
```bash
cd crates/iota-desktop
npx tsc --noUnusedLocals --noUnusedParameters --noEmit
```

**Output:** Generate a consolidated report listing all flagged items with file paths and line numbers.

**Why:** Compiler-backed detection ensures we only remove code that's provably unused. The 367-test suite will catch any breakage.

**How to apply:** Review the report and remove each flagged item. Skip items that are:
- Public API exports (even if unused internally)
- Test helpers used across multiple test files
- Code marked with `#[allow(dead_code)]` for valid reasons (FFI, trait requirements)

### Phase 2: Priority Zone Manual Cleanup

Focus manual review on recently modified files from the desktop daemon integration (last 2 commits):

**Target files:**
- `crates/iota-core/src/daemon/desktop.rs`
- `crates/iota-core/src/daemon/desktop_tests.rs`
- `crates/iota-desktop/src-tauri/src/lib.rs`
- `crates/iota-desktop/src/api.ts`
- `crates/iota-desktop/src/components/ChatWorkbench.tsx`
- `crates/iota-desktop/src/components/ConfigPanel.tsx`
- `crates/iota-desktop/src/components/RightInspector.tsx`
- `crates/iota-desktop/src/types.ts`
- `crates/iota-desktop/src/turnReducer.ts`

**Manual review checklist per file:**
1. Search for commented code blocks: `//.*\n//` (consecutive lines), `/* ... */` (multi-line)
2. Look for debug statements: `println!`, `eprintln!`, `console.log`, `console.debug`
3. Check for unreachable code after `return`, `panic!`, `break`, `continue`
4. Review error handling: remove defensive checks for conditions that can't occur
5. Remove unused imports flagged by IDE/compiler

**Why:** Recent code is where cruft accumulates during active development. Cleaning it now prevents it from spreading.

**How to apply:** For each file, read it fully, apply the checklist, verify tests still pass.

### Phase 3: Pattern-Based Obvious Cases

Use grep/ripgrep to find and remove obvious patterns across the entire codebase:

**Patterns to remove:**
- Commented-out imports: `// use .*` or `// import .*`
- Commented-out function signatures: `// fn .*` or `// function .*`
- Consecutive comment blocks (5+ lines) that look like old code (contain `{`, `}`, `;`)
- Debug macros in non-test code: `dbg!()` outside `#[cfg(test)]`

**Why:** These patterns are safe to remove automatically - they're clearly not documentation.

**How to apply:** 
```bash
rg "^\\s*// (use|import|fn|function|const|let|pub)" --type rust --type ts
# Review matches, remove obvious commented code
```

### Phase 4: Verification

After all cleanup:

1. **Build check:** `cargo build --all-features`
2. **Test suite:** `cargo test` (all 367 tests must pass)
3. **Clippy clean:** `cargo clippy -- -D warnings`
4. **TypeScript check:** `cd crates/iota-desktop && npx tsc --noEmit`
5. **Manual smoke test:** Run `iota` TUI and test basic flows

**Why:** Ensures no functional regressions from the cleanup.

**How to apply:** Run all checks sequentially. If any fail, revert the problematic change and investigate.

## Success Criteria

- All compiler warnings for dead code, unused imports, and unused variables are resolved
- No commented-out code blocks remain in priority zone files
- Test suite passes with no new failures
- Codebase builds cleanly with no warnings
- No functional regressions in manual testing

## Non-Goals

- Refactoring or restructuring code
- Removing "dead" code that's part of public APIs
- Cleaning up documentation or markdown files
- Optimizing algorithms or improving performance
- Removing experimental code in `gefsi/` directory (out of scope)

## Rollback Plan

All changes will be in a single commit with a clear message. If issues arise:
```bash
git revert HEAD
```

The cleanup is purely subtractive - no logic changes - so rollback is safe and complete.
