import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { 
  MessageSquare, 
  Kanban, 
  Settings, 
  Terminal, 
  Send, 
  AlertCircle, 
  User, 
  Plus, 
  ArrowRight,
  RefreshCw,
  Cpu
} from "lucide-react";
import "./App.css";

// Type definitions matching iota-core domain models
interface Board {
  id: number;
  slug: string;
  name: string;
  created_at: number;
}

interface Task {
  id: number;
  board_id: number;
  title: string;
  body: string | null;
  status: string;
  assignee: string | null;
  priority: number;
  tags: string[];
  created_at: number;
}

interface ModelConfig {
  provider?: string;
  name?: string;
  base_url?: string;
  api_key?: string;
}

interface BackendConfig {
  enabled: boolean;
  model?: ModelConfig;
  tool_whitelist?: string[];
}

interface NimiaConfig {
  model?: {
    provider?: string;
    name?: string;
    base_url?: string;
  };
  backends?: Record<string, {
    enabled?: boolean;
    model?: string;
  }>;
  claude_code?: BackendConfig;
  codex?: BackendConfig;
  gemini?: BackendConfig;
  opencode?: BackendConfig;
  hermes?: BackendConfig;
}

function App() {
  const [activeTab, setActiveTab] = useState<"chat" | "kanban" | "config">("kanban");
  const [boards, setBoards] = useState<Board[]>([]);
  const [selectedBoardId, setSelectedBoardId] = useState<number | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  
  // Kanban create task state
  const [newTaskTitle, setNewTaskTitle] = useState("");
  const [newTaskBody, setNewTaskBody] = useState("");
  const [newTaskStatus, setNewTaskStatus] = useState("triage");

  // Chat State
  const [chatBackend, setChatBackend] = useState("gemini");
  const [chatInput, setChatInput] = useState("");
  const [messages, setMessages] = useState<Array<{ sender: "user" | "agent"; text: string; time: string }>>([
    { sender: "agent", text: "您好！我是 Iota Sympantos 智能编程助手。我已经成功链接了本地的 `iota-core` 引擎，准备好协同您开发项目。", time: "10:00" }
  ]);
  const [isTyping, setIsTyping] = useState(false);
  const [currentResponse, setCurrentResponse] = useState("");
  const [pendingApproval, setPendingApproval] = useState<{ id: string; tool_name: string; params: any } | null>(null);
  const [apiKeyRequiredBackend, setApiKeyRequiredBackend] = useState<string | null>(null);
  const [apiKeyValue, setApiKeyValue] = useState("");

  // Config State
  const [config, setConfig] = useState<NimiaConfig | null>(null);

  // Load Kanban Data
  const loadKanbanData = async () => {
    setLoading(true);
    setError(null);
    try {
      const fetchedBoards = await invoke<Board[]>("list_boards");
      setBoards(fetchedBoards);
      if (fetchedBoards.length > 0) {
        const boardId = fetchedBoards[0].id;
        setSelectedBoardId(boardId);
        const fetchedTasks = await invoke<Task[]>("list_tasks", {
          filter: { board_id: boardId }
        });
        setTasks(fetchedTasks);
      }
    } catch (e) {
      console.error(e);
      setError("无法加载看板数据: " + String(e));
    } finally {
      setLoading(false);
    }
  };

  // Load Config Data
  const loadConfigData = async () => {
    try {
      const cfg = await invoke<NimiaConfig>("get_config");
      setConfig(cfg);
    } catch (e) {
      console.error(e);
    }
  };

  useEffect(() => {
    loadKanbanData();
    loadConfigData();

    // Listen for stream chunks from Tauri backend
    const unlistenChunk = listen<string>("chat-stream-chunk", (event) => {
      setIsTyping(false);
      const chunk = event.payload || "";
      setCurrentResponse(prev => prev + chunk);
    });

    // Listen for prompt completion
    const unlistenComplete = listen<{ text: string; events: any[]; timing: any }>("chat-complete", (event) => {
      const reply = event.payload?.text || "会话完成，但未返回文本。";
      setMessages(prev => [
        ...prev, 
        { sender: "agent", text: reply, time: new Date().toLocaleTimeString().slice(0, 5) }
      ]);
      setCurrentResponse("");
      setIsTyping(false);
    });

    // Listen for prompt execution errors
    const unlistenError = listen<string>("chat-error", (event) => {
      const errMsg = event.payload || "未知执行错误";
      setMessages(prev => [
        ...prev, 
        { sender: "agent", text: "运行出错: " + errMsg, time: new Date().toLocaleTimeString().slice(0, 5) }
      ]);
      setCurrentResponse("");
      setIsTyping(false);
    });

    // Listen for tool call approvals
    const unlistenApproval = listen<{ id: string; tool_name: string; params: any }>("approval-requested", (event) => {
      setPendingApproval(event.payload);
    });

    return () => {
      unlistenChunk.then(f => f());
      unlistenComplete.then(f => f());
      unlistenError.then(f => f());
      unlistenApproval.then(f => f());
    };
  }, [selectedBoardId]);

  // Handle task status transition
  const handleTransition = async (taskId: number, newStatus: string) => {
    try {
      await invoke("transition_task", { taskId, toStatus: newStatus });
      // Reload tasks
      if (selectedBoardId !== null) {
        const fetchedTasks = await invoke<Task[]>("list_tasks", {
          filter: { board_id: selectedBoardId }
        });
        setTasks(fetchedTasks);
      }
    } catch (e) {
      alert("转移任务状态失败: " + String(e));
    }
  };

  // Handle task creation
  const handleCreateTask = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newTaskTitle.trim() || selectedBoardId === null) return;

    try {
      await invoke("create_task", {
        req: {
          board_id: selectedBoardId,
          title: newTaskTitle,
          body: newTaskBody || null,
          status: newTaskStatus,
          assignee: "Developer",
          priority: 1,
          tags: ["gui-created"]
        }
      });
      setNewTaskTitle("");
      setNewTaskBody("");
      // Reload tasks
      const fetchedTasks = await invoke<Task[]>("list_tasks", {
        filter: { board_id: selectedBoardId }
      });
      setTasks(fetchedTasks);
    } catch (e) {
      alert("创建任务失败: " + String(e));
    }
  };

  // Send Prompt Message to iota-core via Tauri command
  const handleSendMessage = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!chatInput.trim()) return;

    const userMsg = chatInput;
    setMessages(prev => [...prev, { sender: "user", text: userMsg, time: new Date().toLocaleTimeString().slice(0, 5) }]);
    setChatInput("");
    setIsTyping(true);
    setCurrentResponse(""); // Clear text buffer

    try {
      await invoke("submit_prompt", { prompt: userMsg, backendStr: chatBackend });
    } catch (err) {
      if (String(err) === "API_KEY_REQUIRED") {
        setApiKeyRequiredBackend(chatBackend);
      } else {
        setMessages(prev => [
          ...prev,
          { sender: "agent", text: "发送失败: " + String(err), time: new Date().toLocaleTimeString().slice(0, 5) }
        ]);
      }
      setIsTyping(false);
    }
  };

  // Status columns helper
  const columns = [
    { id: "triage", name: "收件箱 (Triage)", color: "border-slate-500/30 text-slate-400 bg-slate-500/5" },
    { id: "todo", name: "待处理 (Todo)", color: "border-sky-500/30 text-sky-400 bg-sky-500/5" },
    { id: "ready", name: "就绪 (Ready)", color: "border-violet-500/30 text-violet-400 bg-violet-500/5" },
    { id: "running", name: "执行中 (Running)", color: "border-pink-500/30 text-pink-400 bg-pink-500/5 animate-pulse" },
    { id: "blocked", name: "阻塞 (Blocked)", color: "border-rose-500/30 text-rose-400 bg-rose-500/5" },
    { id: "done", name: "已完成 (Done)", color: "border-emerald-500/30 text-emerald-400 bg-emerald-500/5" }
  ];

  return (
    <div className="flex h-screen bg-[#0b0f19] text-gray-100 overflow-hidden font-sans">
      
      {/* Sidebar Navigation */}
      <aside className="w-64 bg-[#070a13] border-r border-white/5 flex flex-col justify-between shrink-0">
        <div>
          {/* Logo Brand */}
          <div className="p-6 border-b border-white/5 flex items-center space-x-3">
            <div className="w-9 h-9 rounded-xl bg-gradient-to-tr from-primary to-purple-600 flex items-center justify-center shadow-lg shadow-primary/20">
              <Cpu className="w-5 h-5 text-white" />
            </div>
            <div>
              <h1 className="font-bold text-lg tracking-wider bg-gradient-to-r from-white to-gray-400 bg-clip-text text-transparent">Iota Sympantos</h1>
              <p className="text-[10px] text-gray-500 tracking-widest uppercase">Desktop Core v0.1.0</p>
            </div>
          </div>

          {/* Navigation Links */}
          <nav className="p-4 space-y-1">
            <button
              onClick={() => setActiveTab("chat")}
              className={`w-full flex items-center space-x-3 px-4 py-3 rounded-xl transition-all duration-200 ${
                activeTab === "chat" 
                  ? "bg-primary/10 text-primary font-medium shadow-inner border border-primary/20" 
                  : "text-gray-400 hover:bg-white/5 hover:text-gray-200"
              }`}
            >
              <MessageSquare className="w-5 h-5" />
              <span>协同问答 (Chat)</span>
            </button>

            <button
              onClick={() => setActiveTab("kanban")}
              className={`w-full flex items-center space-x-3 px-4 py-3 rounded-xl transition-all duration-200 ${
                activeTab === "kanban" 
                  ? "bg-primary/10 text-primary font-medium shadow-inner border border-primary/20" 
                  : "text-gray-400 hover:bg-white/5 hover:text-gray-200"
              }`}
            >
              <Kanban className="w-5 h-5" />
              <span>智能看板 (Kanban)</span>
            </button>

            <button
              onClick={() => setActiveTab("config")}
              className={`w-full flex items-center space-x-3 px-4 py-3 rounded-xl transition-all duration-200 ${
                activeTab === "config" 
                  ? "bg-primary/10 text-primary font-medium shadow-inner border border-primary/20" 
                  : "text-gray-400 hover:bg-white/5 hover:text-gray-200"
              }`}
            >
              <Settings className="w-5 h-5" />
              <span>配置项 (Config)</span>
            </button>
          </nav>
        </div>

        {/* System Status Indicators */}
        <div className="p-4 border-t border-white/5 bg-[#04060b]/50">
          <div className="flex items-center justify-between text-xs mb-2">
            <span className="text-gray-500">Core Runtime</span>
            <span className="flex items-center space-x-1.5 text-emerald-400">
              <span className="w-2 h-2 rounded-full bg-emerald-400 animate-ping"></span>
              <span>已连接</span>
            </span>
          </div>
          <div className="flex items-center justify-between text-xs">
            <span className="text-gray-500">Local Daemon</span>
            <span className="text-gray-400">127.0.0.1:47661</span>
          </div>
        </div>
      </aside>

      {/* Main Panel Viewport */}
      <main className="flex-1 flex flex-col overflow-hidden font-sans">
        
        {/* Chat Interface Tab */}
        {activeTab === "chat" && (
          <div className="flex-1 flex flex-col overflow-hidden bg-gradient-to-b from-[#0e1322] to-[#0a0d16]">
            {/* Header */}
            <header className="p-6 border-b border-white/5 flex items-center justify-between shrink-0">
              <div className="flex items-center space-x-3">
                <MessageSquare className="w-6 h-6 text-primary" />
                <div>
                  <h2 className="font-semibold text-lg">AI 智能编程协作</h2>
                  <p className="text-xs text-gray-500">ACP 协议托管会话</p>
                </div>
              </div>
              
              {/* Backend Picker */}
              <div className="flex items-center space-x-2">
                <span className="text-xs text-gray-500">适配器:</span>
                <select
                  value={chatBackend}
                  onChange={(e) => setChatBackend(e.target.value)}
                  className="bg-white/5 border border-white/10 rounded-lg px-3 py-1 text-xs outline-none cursor-pointer focus:border-primary/50 text-gray-300"
                >
                  <option value="gemini" className="bg-[#0b0f19]">Gemini CLI (gemini)</option>
                  <option value="claude" className="bg-[#0b0f19]">Claude Code (claude)</option>
                  <option value="hermes" className="bg-[#0b0f19]">Hermes Agent (hermes)</option>
                  <option value="codex" className="bg-[#0b0f19]">Codex (codex)</option>
                  <option value="opencode" className="bg-[#0b0f19]">OpenCode (opencode)</option>
                </select>
              </div>
            </header>

            {/* Message Pane */}
            <div className="flex-1 overflow-y-auto p-6 space-y-4">
              {messages.map((msg, index) => (
                <div 
                  key={index}
                  className={`flex ${msg.sender === "user" ? "justify-end" : "justify-start"}`}
                >
                  <div className={`max-w-[70%] rounded-2xl p-4 shadow-md ${
                    msg.sender === "user"
                      ? "bg-gradient-to-tr from-primary to-purple-600 text-white rounded-tr-none"
                      : "bg-[#141b2e] border border-white/5 text-gray-200 rounded-tl-none"
                  }`}>
                    <p className="text-sm leading-relaxed whitespace-pre-wrap">{msg.text}</p>
                    <div className="text-[9px] text-white/50 text-right mt-1.5">{msg.time}</div>
                  </div>
                </div>
              ))}

              {/* Streaming Response Chunk Preview */}
              {currentResponse && (
                <div className="flex justify-start">
                  <div className="max-w-[70%] rounded-2xl p-4 shadow-md bg-[#141b2e] border border-white/5 text-gray-200 rounded-tl-none">
                    <p className="text-sm leading-relaxed whitespace-pre-wrap">{currentResponse}</p>
                    <div className="text-[9px] text-gray-500 text-right mt-1.5">正在输入 (Streaming...)</div>
                  </div>
                </div>
              )}

              {isTyping && (
                <div className="flex justify-start">
                  <div className="bg-[#141b2e] border border-white/5 text-gray-400 rounded-2xl rounded-tl-none p-4 flex items-center space-x-2">
                    <span className="w-1.5 h-1.5 bg-gray-500 rounded-full animate-bounce"></span>
                    <span className="w-1.5 h-1.5 bg-gray-500 rounded-full animate-bounce" style={{ animationDelay: "0.2s" }}></span>
                    <span className="w-1.5 h-1.5 bg-gray-500 rounded-full animate-bounce" style={{ animationDelay: "0.4s" }}></span>
                  </div>
                </div>
              )}
            </div>

            {/* Input Form */}
            <footer className="p-6 border-t border-white/5 bg-[#070a13]/50">
              <form onSubmit={handleSendMessage} className="flex items-center space-x-3">
                <input
                  type="text"
                  value={chatInput}
                  onChange={(e) => setChatInput(e.target.value)}
                  placeholder={`输入您的指令，通过 ${chatBackend} 引擎进行代码开发...`}
                  className="flex-1 bg-white/5 border border-white/10 rounded-xl px-4 py-3.5 text-sm placeholder-gray-600 outline-none focus:border-primary/50 transition duration-150 text-gray-200"
                />
                <button
                  type="submit"
                  disabled={!chatInput.trim() || isTyping}
                  className="p-3.5 rounded-xl bg-primary text-white hover:bg-primary/95 transition duration-150 shadow-lg shadow-primary/20 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <Send className="w-5 h-5" />
                </button>
              </form>
            </footer>
          </div>
        )}

        {/* Kanban Board View */}
        {activeTab === "kanban" && (
          <div className="flex-1 flex flex-col overflow-hidden bg-gradient-to-b from-[#0a0d16] to-[#0e1322]">
            {/* Header */}
            <header className="p-6 border-b border-white/5 flex items-center justify-between shrink-0">
              <div className="flex items-center space-x-3">
                <Kanban className="w-6 h-6 text-primary" />
                <div>
                  <h2 className="font-semibold text-lg">看板控制台 (Kanban)</h2>
                  <p className="text-xs text-gray-500">
                    {boards.length > 0 ? `当前看板: ${boards[0].name}` : "正在连接本地 SQLite 数据仓库..."}
                  </p>
                </div>
              </div>
              
              <div className="flex items-center space-x-3">
                <button
                  onClick={loadKanbanData}
                  className="p-2 bg-white/5 hover:bg-white/10 border border-white/10 rounded-lg text-gray-400 hover:text-gray-200 transition cursor-pointer"
                  title="刷新看板数据"
                >
                  <RefreshCw className={`w-4 h-4 ${loading ? "animate-spin text-primary" : ""}`} />
                </button>
              </div>
            </header>

            {/* Error alerts */}
            {error && (
              <div className="m-6 p-4 bg-rose-500/10 border border-rose-500/20 text-rose-400 rounded-xl flex items-center space-x-3">
                <AlertCircle className="w-5 h-5" />
                <span className="text-sm font-medium">{error}</span>
              </div>
            )}

            {/* Board Columns Grid */}
            <div className="flex-1 overflow-x-auto p-6 flex space-x-4 items-start select-none">
              {columns.map(col => {
                const columnTasks = tasks.filter(t => t.status === col.id);
                return (
                  <div 
                    key={col.id}
                    className="w-80 rounded-2xl bg-[#0d1220]/80 border border-white/5 p-4 flex flex-col max-h-full shrink-0 shadow-xl"
                  >
                    {/* Column Header */}
                    <div className="flex items-center justify-between mb-4 pb-2 border-b border-white/5">
                      <span className="font-medium text-sm flex items-center space-x-2">
                        <span className={`w-2 h-2 rounded-full ${col.id === 'running' ? 'bg-pink-500' : col.id === 'ready' ? 'bg-violet-500' : 'bg-gray-500'}`}></span>
                        <span className="text-gray-300">{col.name}</span>
                      </span>
                      <span className="text-xs text-gray-500 px-2 py-0.5 rounded-full bg-white/5 font-semibold">
                        {columnTasks.length}
                      </span>
                    </div>

                    {/* Task Cards Container */}
                    <div className="space-y-3 overflow-y-auto flex-1 min-h-[150px] pr-1.5">
                      {columnTasks.map(task => (
                        <div 
                          key={task.id}
                          className="bg-[#141b2e] border border-white/5 rounded-xl p-3.5 hover:border-primary/30 transition duration-150 cursor-pointer shadow-md group"
                        >
                          <h4 className="text-sm font-medium text-gray-200 group-hover:text-primary transition duration-150 mb-1">{task.title}</h4>
                          <p className="text-xs text-gray-500 line-clamp-2 leading-relaxed mb-3">{task.body || "无详细描述"}</p>
                          
                          {/* Tags */}
                          {task.tags && task.tags.length > 0 && (
                            <div className="flex flex-wrap gap-1 mb-3">
                              {task.tags.map((tag, tIdx) => (
                                <span key={tIdx} className="text-[9px] bg-white/5 text-gray-400 px-2 py-0.5 rounded-md font-semibold">
                                  {tag}
                                </span>
                              ))}
                            </div>
                          )}

                          {/* Footer Actions */}
                          <div className="flex items-center justify-between border-t border-white/5 pt-2 mt-2">
                            <span className="text-[10px] text-gray-500 flex items-center space-x-1">
                              <User className="w-3 h-3" />
                              <span>{task.assignee || "未分配"}</span>
                            </span>

                            {/* Dropdown status transition selector */}
                            <select
                              value={task.status}
                              onChange={(e) => handleTransition(task.id, e.target.value)}
                              className="bg-[#0b0f19] border border-white/10 rounded px-1.5 py-0.5 text-[10px] text-gray-400 outline-none cursor-pointer focus:border-primary"
                            >
                              <option value="triage">Triage</option>
                              <option value="todo">Todo</option>
                              <option value="ready">Ready</option>
                              <option value="running">Running</option>
                              <option value="blocked">Blocked</option>
                              <option value="done">Done</option>
                            </select>
                          </div>
                        </div>
                      ))}
                      
                      {columnTasks.length === 0 && (
                        <div className="h-28 border border-dashed border-white/5 rounded-xl flex items-center justify-center text-gray-600 text-xs">
                          暂无卡片
                        </div>
                      )}
                    </div>
                  </div>
                );
              })}

              {/* Quick Task Creation Card */}
              <div className="w-80 rounded-2xl bg-[#070a13]/60 border border-dashed border-white/10 p-5 shrink-0 flex flex-col shadow-inner">
                <div className="flex items-center space-x-2 text-primary font-semibold text-sm mb-4">
                  <Plus className="w-5 h-5" />
                  <span>快捷新建任务 (Add Task)</span>
                </div>
                
                <form onSubmit={handleCreateTask} className="space-y-4">
                  <div>
                    <label className="block text-xs text-gray-500 mb-1">任务名称 *</label>
                    <input
                      type="text"
                      required
                      value={newTaskTitle}
                      onChange={(e) => setNewTaskTitle(e.target.value)}
                      placeholder="例如: 编写单元测试..."
                      className="w-full bg-[#0b0f19] border border-white/5 rounded-lg px-3 py-2 text-xs text-gray-200 placeholder-gray-700 outline-none focus:border-primary/50"
                    />
                  </div>
                  <div>
                    <label className="block text-xs text-gray-500 mb-1">描述</label>
                    <textarea
                      value={newTaskBody}
                      onChange={(e) => setNewTaskBody(e.target.value)}
                      placeholder="详细说明..."
                      rows={3}
                      className="w-full bg-[#0b0f19] border border-white/5 rounded-lg px-3 py-2 text-xs text-gray-200 placeholder-gray-700 outline-none focus:border-primary/50 resize-none"
                    />
                  </div>
                  <div>
                    <label className="block text-xs text-gray-500 mb-1">初始列</label>
                    <select
                      value={newTaskStatus}
                      onChange={(e) => setNewTaskStatus(e.target.value)}
                      className="w-full bg-[#0b0f19] border border-white/5 rounded-lg px-3 py-2 text-xs text-gray-400 outline-none cursor-pointer"
                    >
                      <option value="triage">收件箱 (Triage)</option>
                      <option value="todo">待处理 (Todo)</option>
                      <option value="ready">就绪 (Ready)</option>
                    </select>
                  </div>
                  <button
                    type="submit"
                    className="w-full bg-primary hover:bg-primary/95 text-white font-medium py-2 rounded-lg text-xs cursor-pointer shadow-lg shadow-primary/10 flex items-center justify-center space-x-1.5"
                  >
                    <span>创建任务</span>
                    <ArrowRight className="w-3.5 h-3.5" />
                  </button>
                </form>
              </div>
            </div>
          </div>
        )}

        {activeTab === "config" && (() => {
          const activeModel = config?.hermes?.model || config?.gemini?.model || config?.claude_code?.model || config?.opencode?.model || config?.codex?.model;
          const provider = activeModel?.provider || "minimax-cn";
          const modelName = activeModel?.name || "MiniMax-M2.7";
          const baseUrl = activeModel?.base_url || "默认 (Default)";

          return (
            <div className="flex-1 flex flex-col bg-gradient-to-b from-[#0a0d16] to-[#0e1322] p-8 overflow-y-auto">
              {/* Header */}
              <header className="flex items-center space-x-3 mb-8">
                <Settings className="w-6 h-6 text-primary" />
                <div>
                  <h2 className="font-semibold text-lg">系统配置项 (Config)</h2>
                  <p className="text-xs text-gray-500">加载自本地外部配置文件 `~/.i6/nimia.yaml`</p>
                </div>
              </header>

              {/* Model Card Config */}
              <div className="grid grid-cols-1 md:grid-cols-2 gap-6 max-w-4xl">
                <div className="rounded-2xl border border-white/5 bg-[#0d1220]/70 p-6 shadow-xl flex flex-col justify-between">
                  <div>
                    <div className="flex items-center justify-between mb-4">
                      <h3 className="font-semibold text-sm text-gray-300 flex items-center space-x-2">
                        <Cpu className="w-4 h-4 text-primary" />
                        <span>全局推荐模型配置</span>
                      </h3>
                      <span className="text-[10px] px-2 py-0.5 bg-emerald-500/10 text-emerald-400 rounded-full font-medium">已连接</span>
                    </div>
                    
                    <div className="space-y-3 text-xs">
                      <div className="flex justify-between border-b border-white/5 pb-2">
                        <span className="text-gray-500">推荐提供商 (Provider)</span>
                        <span className="text-gray-300 font-mono">{provider}</span>
                      </div>
                      <div className="flex justify-between border-b border-white/5 pb-2">
                        <span className="text-gray-500">推荐模型名 (Model Name)</span>
                        <span className="text-gray-300 font-mono">{modelName}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-gray-500">推荐 Base URL</span>
                        <span className="text-gray-300 font-mono overflow-hidden text-ellipsis whitespace-nowrap max-w-[200px]" title={activeModel?.base_url || "默认 (Default)"}>
                          {baseUrl}
                        </span>
                      </div>
                    </div>
                  </div>
                </div>

                <div className="rounded-2xl border border-white/5 bg-[#0d1220]/70 p-6 shadow-xl">
                  <h3 className="font-semibold text-sm text-gray-300 flex items-center space-x-2 mb-4">
                    <Terminal className="w-4 h-4 text-primary" />
                    <span>后端代理驱动配置</span>
                  </h3>
                  
                  <div className="space-y-3.5">
                    <div className="flex items-center justify-between text-xs">
                      <span className="text-gray-400">Claude Code Adapter</span>
                      <span className={config?.claude_code?.enabled !== false ? "text-emerald-400 font-semibold" : "text-gray-500 font-semibold"}>
                        {config?.claude_code?.enabled !== false ? `Enabled (${config?.claude_code?.model?.name || "Sonnet 4"})` : "Disabled"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between text-xs">
                      <span className="text-gray-400">Gemini CLI Adapter</span>
                      <span className={config?.gemini?.enabled !== false ? "text-emerald-400 font-semibold" : "text-gray-500 font-semibold"}>
                        {config?.gemini?.enabled !== false ? `Enabled (${config?.gemini?.model?.name || "gemini-2.5"})` : "Disabled"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between text-xs">
                      <span className="text-gray-400">Hermes Agent Adapter</span>
                      <span className={config?.hermes?.enabled !== false ? "text-emerald-400 font-semibold" : "text-gray-500 font-semibold"}>
                        {config?.hermes?.enabled !== false ? `Enabled (${config?.hermes?.model?.name || "MiniMax"})` : "Disabled"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between text-xs">
                      <span className="text-gray-400">Codex Adapter</span>
                      <span className={config?.codex?.enabled !== false ? "text-emerald-400 font-semibold" : "text-gray-500 font-semibold"}>
                        {config?.codex?.enabled !== false ? `Enabled (${config?.codex?.model?.name || "gpt-5.5"})` : "Disabled"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between text-xs">
                      <span className="text-gray-400">OpenCode Adapter</span>
                      <span className={config?.opencode?.enabled !== false ? "text-emerald-400 font-semibold" : "text-gray-500 font-semibold"}>
                        {config?.opencode?.enabled !== false ? `Enabled (${config?.opencode?.model?.name || "MiniMax"})` : "Disabled"}
                      </span>
                    </div>
                  </div>
                </div>
              </div>
              
              {/* Raw JSON View */}
              <div className="mt-8 max-w-4xl">
                <h3 className="text-sm font-semibold text-gray-400 mb-3">原始配置文件快照 (JSON format)</h3>
                <div className="bg-[#05070d] rounded-2xl border border-white/5 p-4 font-mono text-xs text-gray-400 overflow-x-auto max-h-64 shadow-inner">
                  <pre>{JSON.stringify(config, null, 2)}</pre>
                </div>
              </div>
            </div>
          );
        })()}
      </main>

      {/* Approval Modal Prompt Popup */}
      {pendingApproval && (
        <div className="fixed inset-0 bg-black/60 backdrop-blur-sm z-50 flex items-center justify-center p-4">
          <div className="w-full max-w-lg rounded-2xl border border-white/10 bg-[#0d1220] p-6 shadow-2xl animate-in fade-in zoom-in duration-200">
            <div className="flex items-center space-x-3 mb-4">
              <AlertCircle className="w-6 h-6 text-rose-400 animate-bounce" />
              <div>
                <h3 className="font-bold text-lg text-gray-100">工具调用许可确认</h3>
                <p className="text-xs text-gray-500">ACP 进程请求执行高风险或敏感操作</p>
              </div>
            </div>
            
            <div className="bg-white/5 rounded-xl p-4 border border-white/5 font-mono text-xs mb-6 text-gray-300 space-y-2">
              <div>
                <span className="text-gray-500">工具名称 (Tool):</span>{" "}
                <span className="text-primary font-semibold">{pendingApproval.tool_name}</span>
              </div>
              <div>
                <span className="text-gray-500">调用参数 (Arguments):</span>
                <pre className="mt-1 bg-black/35 rounded p-2 overflow-x-auto text-[10px] text-gray-400">
                  {JSON.stringify(pendingApproval.params, null, 2)}
                </pre>
              </div>
            </div>

            <div className="flex space-x-3 justify-end">
              <button
                onClick={async () => {
                  try {
                    await invoke("handle_approval", { reqId: pendingApproval.id, approved: false });
                    setPendingApproval(null);
                  } catch (e) {
                    alert(e);
                  }
                }}
                className="px-5 py-2.5 rounded-xl bg-white/5 hover:bg-white/10 border border-white/5 text-gray-300 font-medium text-sm transition cursor-pointer"
              >
                拒绝阻止 (Deny)
              </button>
              <button
                onClick={async () => {
                  try {
                    await invoke("handle_approval", { reqId: pendingApproval.id, approved: true });
                    setPendingApproval(null);
                  } catch (e) {
                    alert(e);
                  }
                }}
                className="px-5 py-2.5 rounded-xl bg-primary hover:bg-primary/95 text-white font-medium text-sm transition cursor-pointer shadow-lg shadow-primary/20"
              >
                允许执行 (Approve)
              </button>
            </div>
          </div>
        </div>
      )}
      {/* API Key Input Modal Prompt */}
      {apiKeyRequiredBackend && (
        <div className="fixed inset-0 bg-black/60 backdrop-blur-sm z-50 flex items-center justify-center p-4">
          <div className="w-full max-w-md rounded-2xl border border-white/10 bg-[#0d1220] p-6 shadow-2xl animate-in fade-in zoom-in duration-200">
            <div className="flex items-center space-x-3 mb-4">
              <Settings className="w-6 h-6 text-primary animate-spin" />
              <div>
                <h3 className="font-bold text-lg text-gray-100">配置 API Key</h3>
                <p className="text-xs text-gray-500">检测到选择的 AI 后端尚未配置 API 密钥</p>
              </div>
            </div>
            
            <p className="text-xs text-gray-400 mb-4 leading-relaxed">
              运行 <strong>{apiKeyRequiredBackend}</strong> 需要提供对应的 API 密钥。请输入您的 Key，系统将自动写入本地的 <code>~/.i6/nimia.yaml</code> 配置文件。
            </p>

            <form onSubmit={async (e) => {
              e.preventDefault();
              if (!apiKeyValue.trim()) return;
              try {
                // Save key to nimia.yaml
                await invoke("save_api_key", { backendStr: apiKeyRequiredBackend, apiKey: apiKeyValue });
                const savedBackend = apiKeyRequiredBackend;
                setApiKeyRequiredBackend(null);
                setApiKeyValue("");
                // Re-submit the message!
                setIsTyping(true);
                await invoke("submit_prompt", { prompt: messages[messages.length - 1]?.text || "", backendStr: savedBackend });
              } catch (err) {
                alert("保存失败: " + String(err));
                setIsTyping(false);
              }
            }}>
              <div className="mb-6">
                <input
                  type="password"
                  required
                  value={apiKeyValue}
                  onChange={(e) => setApiKeyValue(e.target.value)}
                  placeholder="输入 API Key (如 sk-... 或 AIza...)"
                  className="w-full bg-[#0b0f19] border border-white/10 rounded-lg px-4 py-2.5 text-sm text-gray-200 placeholder-gray-600 outline-none focus:border-primary/50"
                />
              </div>

              <div className="flex space-x-3 justify-end">
                <button
                  type="button"
                  onClick={() => {
                    setApiKeyRequiredBackend(null);
                    setApiKeyValue("");
                    setIsTyping(false);
                    setMessages(prev => [
                      ...prev,
                      { sender: "agent", text: "已取消配置，无法继续启动 ACP 后端会话。", time: new Date().toLocaleTimeString().slice(0, 5) }
                    ]);
                  }}
                  className="px-4 py-2 rounded-lg bg-white/5 hover:bg-white/10 border border-white/5 text-gray-300 text-xs transition cursor-pointer"
                >
                  取消取消 (Cancel)
                </button>
                <button
                  type="submit"
                  className="px-4 py-2 rounded-lg bg-primary hover:bg-primary/95 text-white text-xs transition cursor-pointer shadow-lg shadow-primary/20"
                >
                  保存并继续 (Save & Continue)
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
