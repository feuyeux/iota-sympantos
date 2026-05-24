import { useEffect, useState } from "react";
import { getMemoryContextSnapshot } from "../api";
import type { DesktopMemoryContextSnapshot, DesktopMemoryRecord } from "../types";
import {
  Search,
  Database,
  Cpu,
  Layers,
  ChevronRight,
  Copy,
  Check,
  AlertCircle,
  RefreshCw,
  Clock,
  Terminal,
  Sliders
} from "lucide-react";

export type MemoryContextMode = "memory" | "context";

type MemoryContextWorkspaceProps = {
  mode?: MemoryContextMode;
};

export function MemoryContextWorkspace({ mode = "memory" }: MemoryContextWorkspaceProps) {
  const [scopeMode, setScopeMode] = useState<"workspace" | "all">("workspace");
  const [snapshot, setSnapshot] = useState<DesktopMemoryContextSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedBucket, setSelectedBucket] = useState<string>("identity");
  const [selectedRecord, setSelectedRecord] = useState<DesktopMemoryRecord | null>(null);
  const [copiedRecordId, setCopiedRecordId] = useState<string | null>(null);
  const [copiedCapsule, setCopiedCapsule] = useState(false);
  const [fullCapsuleExpanded, setFullCapsuleExpanded] = useState(false);

  const fetchSnapshot = async () => {
    setLoading(true);
    setError(null);
    try {
      const snap = await getMemoryContextSnapshot(scopeMode);
      setSnapshot(snap);
      
      // Keep selected bucket, or fallback to identity if not exist
      const records = snap.memory[selectedBucket as keyof typeof snap.memory] || [];
      if (records.length > 0) {
        setSelectedRecord(records[0]);
      } else {
        setSelectedRecord(null);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchSnapshot();
  }, [scopeMode]);

  // Auto-select first record when switching buckets or filtering
  useEffect(() => {
    if (snapshot) {
      const records = snapshot.memory[selectedBucket as keyof typeof snapshot.memory] || [];
      const filtered = records.filter(r =>
        r.content.toLowerCase().includes(searchQuery.toLowerCase())
      );
      if (filtered.length > 0) {
        setSelectedRecord(filtered[0]);
      } else {
        setSelectedRecord(null);
      }
    }
  }, [selectedBucket, searchQuery, snapshot]);

  const handleCopyRecord = (record: DesktopMemoryRecord) => {
    navigator.clipboard.writeText(record.content);
    setCopiedRecordId(record.id);
    setTimeout(() => setCopiedRecordId(null), 2000);
  };

  const handleCopyCapsule = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopiedCapsule(true);
    setTimeout(() => setCopiedCapsule(false), 2000);
  };

  const formatDate = (ts: number) => {
    if (ts <= 0) return "Never";
    return new Date(ts * 1000).toLocaleString();
  };

  const bucketsList = [
    { id: "identity", label: "Identity", count: snapshot?.memory_summary.identity || 0, color: "text-magenta-400 bg-magenta-500/10 border-magenta-500/20" },
    { id: "preference", label: "Preference", count: snapshot?.memory_summary.preference || 0, color: "text-amber-400 bg-amber-500/10 border-amber-500/20" },
    { id: "strategic", label: "Strategic", count: snapshot?.memory_summary.strategic || 0, color: "text-cyan-400 bg-cyan-500/10 border-cyan-500/20" },
    { id: "domain", label: "Domain", count: snapshot?.memory_summary.domain || 0, color: "text-emerald-400 bg-emerald-500/10 border-emerald-500/20" },
    { id: "procedural", label: "Procedural", count: snapshot?.memory_summary.procedural || 0, color: "text-purple-400 bg-purple-500/10 border-purple-500/20" },
    { id: "episodic", label: "Episodic", count: snapshot?.memory_summary.episodic || 0, color: "text-indigo-400 bg-indigo-500/10 border-indigo-500/20" },
  ];

  const currentRecords = snapshot ? (snapshot.memory[selectedBucket as keyof typeof snapshot.memory] || []) : [];
  const filteredRecords = currentRecords.filter(r =>
    r.content.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const isMemoryMode = mode === "memory";

  return (
    <div className="flex flex-col h-full bg-zinc-950 text-zinc-100 overflow-hidden font-sans">
      {/* Top Header */}
      <header className="flex flex-wrap items-center justify-between gap-4 border-b border-zinc-800 bg-zinc-900/60 px-6 py-4 backdrop-blur-md">
        <div className="flex items-center gap-3">
          <div className="rounded-lg bg-primary/10 p-2 text-primary border border-primary/20">
            {isMemoryMode ? <Database className="h-5 w-5" /> : <Cpu className="h-5 w-5" />}
          </div>
          <div>
            <h1 className="text-base font-bold tracking-tight text-white flex items-center gap-2">
              {isMemoryMode ? "Persistent Memory" : "Runtime Context"}
              <span className="text-[10px] uppercase font-semibold tracking-wider bg-zinc-800 text-zinc-400 px-2 py-0.5 rounded border border-zinc-700">
                Read-Only
              </span>
            </h1>
            <p className="text-xs text-zinc-400">
              {isMemoryMode
                ? "Inspect persistent identity, preference, strategic, domain, procedural, and episodic memories."
                : "Inspect the captured runtime context capsule and Context Fabric configuration."}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-3">
          {/* Scope Selection */}
          <div className="flex rounded-lg bg-zinc-800/80 p-1 border border-zinc-700">
            <button
              onClick={() => setScopeMode("workspace")}
              className={`rounded-md px-3 py-1.5 text-xs font-medium transition-all ${
                scopeMode === "workspace"
                  ? "bg-primary text-white shadow-lg shadow-primary/25"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              Workspace Scope
            </button>
            <button
              onClick={() => setScopeMode("all")}
              className={`rounded-md px-3 py-1.5 text-xs font-medium transition-all ${
                scopeMode === "all"
                  ? "bg-primary text-white shadow-lg shadow-primary/25"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              All Scope
            </button>
          </div>

          {/* Refresh Button */}
          <button
            onClick={fetchSnapshot}
            disabled={loading}
            className="flex items-center justify-center rounded-lg border border-zinc-700 bg-zinc-800/60 p-2 text-zinc-400 hover:bg-zinc-800 hover:text-white transition-all disabled:opacity-50"
            title="Refresh Snapshot"
          >
            <RefreshCw className={`h-4 w-4 ${loading ? "animate-spin text-primary" : ""}`} />
          </button>
        </div>
      </header>

      {/* Errors Notification */}
      {snapshot?.errors && snapshot.errors.length > 0 && (
        <div className="bg-red-500/10 border-b border-red-500/20 px-6 py-2.5 flex items-center gap-3 text-xs text-red-300">
          <AlertCircle className="h-4 w-4 shrink-0 text-red-400" />
          <div className="flex-1 truncate">
            {snapshot.errors.map((err, idx) => (
              <span key={idx} className="mr-4">
                <strong>[{err.area}]</strong> {err.message}
              </span>
            ))}
          </div>
        </div>
      )}

      {error && (
        <div className="bg-red-500/10 border-b border-red-500/20 px-6 py-4 flex items-center gap-3 text-sm text-red-300">
          <AlertCircle className="h-5 w-5 shrink-0 text-red-400" />
          <div>
            <h4 className="font-semibold text-white">Error loading snapshot</h4>
            <p className="text-xs text-red-400/80 mt-0.5">{error}</p>
          </div>
        </div>
      )}

      <div className="flex-1 overflow-hidden">
        
        {isMemoryMode && (
        <section className="flex flex-col h-full overflow-hidden bg-zinc-950">
          <div className="flex items-center justify-between border-b border-zinc-900 bg-zinc-900/10 px-4 py-3">
            <span className="text-xs font-semibold uppercase tracking-wider text-zinc-400 flex items-center gap-1.5">
              <Database className="h-3.5 w-3.5 text-primary" /> Persistent Memory
            </span>
            <span className="text-[10px] text-zinc-500">
              CWD: {snapshot?.cwd || "Unknown"}
            </span>
          </div>

          <div className="flex flex-1 overflow-hidden">
            {/* Buckets Sidebar */}
            <div className="w-40 border-r border-zinc-900 bg-zinc-900/15 flex flex-col gap-1 p-2 overflow-y-auto">
              {bucketsList.map((bucket) => {
                const isSelected = selectedBucket === bucket.id;
                return (
                  <button
                    key={bucket.id}
                    onClick={() => {
                      setSelectedBucket(bucket.id);
                      setSearchQuery("");
                    }}
                    className={`group w-full flex items-center justify-between rounded-lg px-3 py-2.5 text-left text-xs font-medium transition-all ${
                      isSelected
                        ? "bg-zinc-800 text-white shadow-sm border border-white/5"
                        : "text-zinc-400 hover:bg-zinc-900/40 hover:text-zinc-200"
                    }`}
                  >
                    <span className="truncate">{bucket.label}</span>
                    <span className={`rounded-full px-1.5 py-0.5 text-[10px] font-bold ${
                      isSelected ? "bg-primary text-white" : "bg-zinc-800 text-zinc-500 group-hover:text-zinc-300 group-hover:bg-zinc-800/80"
                    }`}>
                      {bucket.count}
                    </span>
                  </button>
                );
              })}
            </div>

            {/* Records Column */}
            <div className="flex-1 flex flex-col h-full overflow-hidden bg-zinc-950">
              {/* Search Bar */}
              <div className="p-3 border-b border-zinc-900">
                <div className="relative">
                  <Search className="absolute left-3 top-2.5 h-3.5 w-3.5 text-zinc-500" />
                  <input
                    type="text"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder={`Search ${selectedBucket} memory...`}
                    className="w-full rounded-md border border-zinc-800 bg-zinc-900/40 py-2 pl-9 pr-4 text-xs text-zinc-200 placeholder-zinc-500 outline-none focus:border-primary focus:bg-zinc-900/80 transition-all"
                  />
                </div>
              </div>

              {/* Record List */}
              <div className="flex-1 overflow-y-auto divide-y divide-zinc-900/50">
                {loading ? (
                  <div className="space-y-3 p-4">
                    <div className="h-10 bg-zinc-900/50 rounded animate-pulse" />
                    <div className="h-10 bg-zinc-900/50 rounded animate-pulse" />
                    <div className="h-10 bg-zinc-900/50 rounded animate-pulse" />
                  </div>
                ) : filteredRecords.length === 0 ? (
                  <div className="flex flex-col items-center justify-center p-8 text-center h-48">
                    <Database className="h-8 w-8 text-zinc-600 mb-2 stroke-1" />
                    <span className="text-xs text-zinc-400 font-medium">No records found</span>
                    <span className="text-[10px] text-zinc-600 mt-1">
                      {searchQuery ? "Try resetting search query" : `No persistent ${selectedBucket} memory yet`}
                    </span>
                  </div>
                ) : (
                  filteredRecords.map((record) => {
                    const isSelected = selectedRecord?.id === record.id;
                    return (
                      <button
                        key={record.id}
                        onClick={() => setSelectedRecord(record)}
                        className={`w-full text-left p-3.5 flex flex-col gap-2 transition-all ${
                          isSelected
                            ? "bg-zinc-900/50 border-l-2 border-primary"
                            : "hover:bg-zinc-900/20"
                        }`}
                      >
                        <div className="flex items-center justify-between w-full">
                          <span className="text-[10px] font-mono text-zinc-500 truncate max-w-[120px]">
                            {record.id}
                          </span>
                          <span className="text-[10px] font-medium bg-zinc-900 text-zinc-400 px-1.5 py-0.5 rounded border border-zinc-800">
                            Conf: {(record.confidence * 100).toFixed(0)}%
                          </span>
                        </div>
                        <p className="text-xs text-zinc-300 line-clamp-2 leading-relaxed">
                          {record.content}
                        </p>
                        <div className="flex items-center justify-between text-[10px] text-zinc-500">
                          <span>Facet: {record.facet || "None"}</span>
                          <span>Scope: {record.scope}</span>
                        </div>
                      </button>
                    );
                  })
                )}
              </div>

              {/* Record Detail Panel */}
              {selectedRecord && (
                <div className="border-t border-zinc-900 bg-zinc-900/30 p-4 flex flex-col gap-3 shrink-0">
                  <div className="flex items-start justify-between">
                    <div className="flex flex-col gap-0.5">
                      <span className="text-[10px] uppercase font-bold text-zinc-500 tracking-wider">Selected Memory Detail</span>
                      <span className="text-xs font-mono text-zinc-400 font-semibold">{selectedRecord.id}</span>
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => handleCopyRecord(selectedRecord)}
                        className="p-1.5 rounded bg-zinc-850 hover:bg-zinc-800 text-zinc-400 hover:text-white transition-all border border-zinc-800"
                        title="Copy Content"
                      >
                        {copiedRecordId === selectedRecord.id ? (
                          <Check className="h-3.5 w-3.5 text-emerald-400" />
                        ) : (
                          <Copy className="h-3.5 w-3.5" />
                        )}
                      </button>
                    </div>
                  </div>

                  <div className="bg-zinc-950/80 border border-zinc-900 rounded-lg p-3 text-xs text-zinc-300 font-mono leading-relaxed max-h-48 overflow-y-auto whitespace-pre-wrap">
                    {selectedRecord.content}
                  </div>

                  <div className="grid grid-cols-2 md:grid-cols-4 gap-3 text-[10px]">
                    <div className="bg-zinc-900/40 p-2 rounded border border-zinc-900">
                      <div className="text-zinc-500">Confidence</div>
                      <div className="text-zinc-300 font-semibold mt-0.5">{(selectedRecord.confidence * 100).toFixed(0)}%</div>
                    </div>
                    <div className="bg-zinc-900/40 p-2 rounded border border-zinc-900">
                      <div className="text-zinc-500">Facet / Type</div>
                      <div className="text-zinc-300 font-semibold mt-0.5 truncate">{selectedRecord.facet || "None"} / {selectedRecord.type}</div>
                    </div>
                    <div className="bg-zinc-900/40 p-2 rounded border border-zinc-900">
                      <div className="text-zinc-500">Updated</div>
                      <div className="text-zinc-300 font-semibold mt-0.5 truncate" title={formatDate(selectedRecord.updated_at)}>
                        {formatDate(selectedRecord.updated_at).split(" ")[0]}
                      </div>
                    </div>
                    <div className="bg-zinc-900/40 p-2 rounded border border-zinc-900">
                      <div className="text-zinc-500">Expires</div>
                      <div className="text-zinc-300 font-semibold mt-0.5 truncate" title={formatDate(selectedRecord.expires_at)}>
                        {selectedRecord.expires_at > 0 ? formatDate(selectedRecord.expires_at).split(" ")[0] : "Never"}
                      </div>
                    </div>
                  </div>
                </div>
              )}
            </div>
          </div>
        </section>
        )}

        {!isMemoryMode && (
        <section className="flex flex-col h-full overflow-hidden bg-zinc-950">
          <div className="flex items-center justify-between border-b border-zinc-900 bg-zinc-900/10 px-4 py-3">
            <span className="text-xs font-semibold uppercase tracking-wider text-zinc-400 flex items-center gap-1.5">
              <Cpu className="h-3.5 w-3.5 text-primary" /> Runtime Context Capsule
            </span>
            <span className="text-[10px] text-zinc-500">
              State: {snapshot?.context_engine.enabled ? "Active" : "Disabled"}
            </span>
          </div>

          <div className="flex-1 overflow-y-auto p-4 flex flex-col gap-4">
            
            {/* Engine configuration state */}
            {snapshot?.context_engine && (
              <div className="rounded-lg border border-zinc-900 bg-zinc-900/20 p-3 flex flex-col gap-2">
                <div className="flex items-center justify-between text-xs">
                  <span className="font-semibold text-zinc-300 flex items-center gap-1.5">
                    <Sliders className="h-3.5 w-3.5 text-zinc-500" /> Context Fabric Config
                  </span>
                  <span className={`px-2 py-0.5 rounded text-[10px] font-bold ${
                    snapshot.context_engine.enabled ? "bg-emerald-500/10 text-emerald-400 border border-emerald-500/20" : "bg-red-500/10 text-red-400 border border-red-500/20"
                  }`}>
                    {snapshot.context_engine.enabled ? "ENABLED" : "DISABLED"}
                  </span>
                </div>
                {snapshot.context_engine.memory_db && (
                  <div className="text-[10px] text-zinc-500 flex items-center gap-1">
                    <Database className="h-3 w-3 shrink-0" />
                    <span className="truncate">DB: {snapshot.context_engine.memory_db}</span>
                  </div>
                )}
                
                {/* Budget Metadata */}
                <div className="grid grid-cols-5 gap-2 mt-1">
                  {Object.entries(snapshot.context_engine.budgets).map(([key, value]) => {
                    const label = key.replace("_chars", "");
                    return (
                      <div key={key} className="bg-zinc-900/60 p-1.5 rounded border border-zinc-800 text-center">
                        <div className="text-[8px] text-zinc-500 uppercase truncate">{label}</div>
                        <div className="text-[10px] font-semibold mt-0.5 text-zinc-300">{(value / 1000).toFixed(0)}k</div>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}

            {loading ? (
              <div className="space-y-4">
                <div className="h-16 bg-zinc-900/50 rounded animate-pulse" />
                <div className="h-32 bg-zinc-900/50 rounded animate-pulse" />
              </div>
            ) : !snapshot?.runtime_context ? (
              <div className="flex flex-col items-center justify-center p-8 text-center flex-1 min-h-[300px]">
                <Cpu className="h-12 w-12 text-zinc-700 mb-3 stroke-1 animate-pulse" />
                <h3 className="text-sm font-semibold text-white">No Turn Context Yet</h3>
                <p className="text-xs text-zinc-500 mt-2 max-w-sm leading-relaxed">
                  No prompt context capsule has been captured in the current session. 
                  Start a conversation in the Chat Workspace to record a context turn.
                </p>
                <div className="mt-4 flex items-center gap-1.5 text-[10px] text-zinc-600 bg-zinc-900/40 px-3 py-1.5 rounded border border-zinc-900">
                  <Terminal className="h-3 w-3" />
                  <span>Captured in-memory only, lost on daemon restart</span>
                </div>
              </div>
            ) : (
              <div className="flex flex-col gap-4">
                {/* Turn Metadata */}
                <div className="rounded-lg border border-zinc-900 bg-zinc-900/40 p-4 flex flex-col gap-3">
                  <div className="flex items-center justify-between border-b border-zinc-900 pb-2">
                    <span className="text-xs font-bold text-white flex items-center gap-1.5">
                      <Clock className="h-3.5 w-3.5 text-primary" /> Turn Metadata
                    </span>
                    <span className="text-[10px] text-zinc-500">{formatDate(snapshot.runtime_context.created_at)}</span>
                  </div>

                  <div className="grid grid-cols-2 md:grid-cols-3 gap-3 text-xs">
                    <div>
                      <div className="text-zinc-500 text-[10px]">Turn ID</div>
                      <div className="font-mono text-zinc-300 font-semibold truncate mt-0.5">{snapshot.runtime_context.turn_id}</div>
                    </div>
                    <div>
                      <div className="text-zinc-500 text-[10px]">Backend</div>
                      <div className="font-semibold text-zinc-300 capitalize mt-0.5">{snapshot.runtime_context.backend}</div>
                    </div>
                    <div>
                      <div className="text-zinc-500 text-[10px]">Model</div>
                      <div className="font-semibold text-zinc-300 truncate mt-0.5">{snapshot.runtime_context.model || "Default"}</div>
                    </div>
                    <div className="col-span-2">
                      <div className="text-zinc-500 text-[10px]">Session ID</div>
                      <div className="font-mono text-zinc-400 truncate mt-0.5">{snapshot.runtime_context.session_id}</div>
                    </div>
                  </div>
                </div>

                {/* Section Summaries */}
                <div className="flex flex-col gap-2">
                  <span className="text-xs font-semibold text-zinc-400 uppercase tracking-wider flex items-center gap-1.5 px-1">
                    <Layers className="h-3.5 w-3.5 text-zinc-500" /> Capsule Sections ({snapshot.runtime_context.sections.length})
                  </span>

                  {snapshot.runtime_context.sections.length === 0 ? (
                    <div className="text-xs text-zinc-500 italic p-3 border border-zinc-900 rounded bg-zinc-900/10">
                      No XML context sections parsed from this capsule.
                    </div>
                  ) : (
                    <div className="space-y-2">
                      {snapshot.runtime_context.sections.map((section) => (
                        <div
                          key={section.name}
                          className="group rounded-lg border border-zinc-900 bg-zinc-900/20 p-3 hover:bg-zinc-900/40 hover:border-zinc-800 transition-all"
                        >
                          <div className="flex items-center justify-between text-xs">
                            <span className="font-mono font-bold text-primary flex items-center gap-1">
                              &lt;{section.name}&gt;
                            </span>
                            <span className="text-[10px] text-zinc-500 bg-zinc-900 px-1.5 py-0.5 rounded border border-zinc-800 group-hover:text-zinc-300">
                              {section.chars.toLocaleString()} chars
                            </span>
                          </div>
                          {section.preview && (
                            <p className="mt-2 text-xs text-zinc-400 leading-relaxed font-sans line-clamp-2 bg-zinc-950/40 p-2 rounded border border-zinc-900/60 font-mono">
                              {section.preview}
                            </p>
                          )}
                        </div>
                      ))}
                    </div>
                  )}
                </div>

                {/* Full Capsule text viewer */}
                <div className="rounded-lg border border-zinc-900 overflow-hidden bg-zinc-900/10">
                  <button
                    onClick={() => setFullCapsuleExpanded(!fullCapsuleExpanded)}
                    className="w-full flex items-center justify-between bg-zinc-900/40 px-4 py-3 hover:bg-zinc-900/60 transition-all"
                  >
                    <span className="text-xs font-semibold text-zinc-300 flex items-center gap-2">
                      <Terminal className="h-3.5 w-3.5 text-zinc-500" />
                      Full Raw Context Capsule
                    </span>
                    <span className="text-[10px] text-zinc-500 flex items-center gap-1">
                      {fullCapsuleExpanded ? "Collapse" : "Expand"}
                      <ChevronRight className={`h-3 w-3 transform transition-transform ${fullCapsuleExpanded ? "rotate-90" : ""}`} />
                    </span>
                  </button>

                  {fullCapsuleExpanded && (
                    <div className="border-t border-zinc-900 bg-zinc-950 flex flex-col">
                      <div className="flex items-center justify-between px-4 py-2 border-b border-zinc-900 bg-zinc-900/20">
                        <span className="text-[10px] text-zinc-500 font-mono">
                          Size: {snapshot.runtime_context.capsule_text.length.toLocaleString()} characters
                        </span>
                        <button
                          onClick={() => handleCopyCapsule(snapshot.runtime_context!.capsule_text)}
                          className="flex items-center gap-1 px-2 py-1 rounded bg-zinc-800 hover:bg-zinc-700 text-[10px] text-zinc-300 border border-zinc-700 transition-all"
                        >
                          {copiedCapsule ? (
                            <>
                              <Check className="h-3 w-3 text-emerald-400" />
                              <span className="text-emerald-400">Copied</span>
                            </>
                          ) : (
                            <>
                              <Copy className="h-3 w-3" />
                              <span>Copy Capsule</span>
                            </>
                          )}
                        </button>
                      </div>
                      <pre className="p-4 text-xs text-zinc-400 font-mono overflow-auto max-h-96 leading-relaxed whitespace-pre-wrap select-text selection:bg-primary/25">
                        {snapshot.runtime_context.capsule_text}
                      </pre>
                    </div>
                  )}
                </div>

              </div>
            )}
          </div>
        </section>
        )}

      </div>
    </div>
  );
}
