import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

test("MemoryContextWorkspace is hosted by the right inspector, not the central workbench", () => {
  const workbench = readFileSync("src/components/ChatWorkbench.tsx", "utf8");
  const inspector = readFileSync("src/components/RightInspector.tsx", "utf8");

  assert.equal(workbench.includes("MemoryContextWorkspace"), false);
  assert.equal(workbench.includes('view === "memory"'), false);
  assert.equal(inspector.includes("MemoryContextWorkspace"), true);
  assert.equal(inspector.includes("Observability"), true);
  assert.equal(inspector.includes("Memory / Context"), false);
  assert.match(inspector, />\s*Memory\s*</);
  assert.match(inspector, />\s*Context\s*</);
  assert.equal(inspector.includes('mode="memory"'), true);
  assert.equal(inspector.includes('mode="context"'), true);
});
