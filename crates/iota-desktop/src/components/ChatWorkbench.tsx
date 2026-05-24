import { useEffect, useMemo, useReducer, useState } from "react";
import { Cpu, Send } from "lucide-react";
import { listenDaemonMessages, submitPrompt } from "../api";
import { initialTurnsState, turnsReducer } from "../turnReducer";
import { RightInspector } from "./RightInspector";
import { ConfigPanel } from "./ConfigPanel";

const BACKENDS = ["gemini", "claude", "hermes", "codex", "opencode"];

export function ChatWorkbench() {
  const [state, dispatch] = useReducer(turnsReducer, initialTurnsState);
  const [backend, setBackend] = useState("gemini");
  const [input, setInput] = useState("");
  const [view, setView] = useState<"chat" | "config">("chat");
  const activeTurn = state.activeTurnId ? state.turns[state.activeTurnId] : undefined;

  useEffect(() => {
    let disposed = false;
    listenDaemonMessages((message) => {
      if (!disposed) dispatch({ type: "daemon_message", message });
    });
    return () => {
      disposed = true;
    };
  }, []);

  const transcript = useMemo(() => state.order.map((id) => state.turns[id]), [state.order, state.turns]);

  async function onSubmit(event: React.FormEvent) {
    event.preventDefault();
    const prompt = input.trim();
    if (!prompt) return;
    setInput("");
    const turnId = await submitPrompt(prompt, backend);
    dispatch({
      type: "turn_started",
      turnId,
      backend,
      cwd: "",
      prompt,
    });
  }

  return (
    <div className="flex h-screen bg-[#0b0f19] text-gray-100">
      <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center justify-between border-b border-white/10 bg-[#070a13] px-5 py-3">
          <div className="flex items-center gap-3">
            <div className="flex h-9 w-9 items-center justify-center rounded-md bg-primary">
              <Cpu className="h-5 w-5 text-white" />
            </div>
            <div>
              <h1 className="text-sm font-semibold">Iota Desktop</h1>
              <p className="text-xs text-gray-500">Daemon-first local workbench</p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <button
              className={`rounded px-3 py-1.5 text-xs font-medium hover:bg-white/10 transition-colors ${
                view === "chat" ? "bg-primary text-white" : "text-gray-300"
              }`}
              onClick={() => setView("chat")}
            >
              Chat
            </button>
            <button
              className={`rounded px-3 py-1.5 text-xs font-medium hover:bg-white/10 transition-colors ${
                view === "config" ? "bg-primary text-white" : "text-gray-300"
              }`}
              onClick={() => setView("config")}
            >
              Config
            </button>
            <select
              value={backend}
              onChange={(event) => setBackend(event.target.value)}
              className="rounded-md border border-white/10 bg-white/[0.04] px-3 py-2 text-xs text-gray-200 ml-4 focus:outline-none focus:border-primary"
            >
              {BACKENDS.map((item) => (
                <option key={item} value={item} className="bg-[#0b0f19]">
                  {item}
                </option>
              ))}
            </select>
          </div>
        </header>

        {view === "chat" ? (
          <>
            <div className="flex-1 overflow-y-auto p-5">
              {transcript.length === 0 ? (
                <div className="flex h-full items-center justify-center text-sm text-gray-500">
                  Send a prompt to begin coding with {backend}
                </div>
              ) : null}
              {transcript.map((turn) => (
                <div key={turn.id} className="mb-6">
                  <div className="mb-2 flex justify-end">
                    <div className="max-w-[72ch] rounded-md bg-primary px-4 py-3 text-sm text-white">
                      {turn.userPrompt}
                    </div>
                  </div>
                  <div className="flex justify-start">
                    <div className="max-w-[88ch] rounded-md border border-white/10 bg-white/[0.04] px-4 py-3 text-sm leading-6 text-gray-200 whitespace-pre-wrap">
                      {turn.assistantText || (turn.status === "failed" ? turn.error : (turn.status === "queued" ? "Queued..." : "Running..."))}
                    </div>
                  </div>
                </div>
              ))}
            </div>

            <form onSubmit={onSubmit} className="border-t border-white/10 bg-[#070a13] p-4">
              <div className="flex gap-3">
                <textarea
                  value={input}
                  onChange={(event) => setInput(event.target.value)}
                  rows={3}
                  className="min-h-[76px] flex-1 resize-none rounded-md border border-white/10 bg-white/[0.04] px-3 py-2 text-sm text-gray-100 outline-none focus:border-primary"
                  placeholder={`Send a prompt through ${backend}`}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      onSubmit(e);
                    }
                  }}
                />
                <button
                  type="submit"
                  disabled={!input.trim()}
                  className="flex h-[76px] w-12 items-center justify-center rounded-md bg-primary text-white disabled:opacity-50"
                >
                  <Send className="h-5 w-5" />
                </button>
              </div>
            </form>
          </>
        ) : (
          <ConfigPanel />
        )}
      </main>

      <RightInspector
        turn={activeTurn}
        onApprovalDecision={(approvalId, approved) =>
          dispatch({ type: "approval_decision", approvalId, approved })
        }
      />
    </div>
  );
}
