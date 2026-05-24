import { useEffect, useState } from "react";
import { getConfig, saveBackendModel } from "../api";
import type { DesktopConfigSnapshot } from "../types";

export function ConfigPanel() {
  const [config, setConfig] = useState<DesktopConfigSnapshot | null>(null);
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({});

  useEffect(() => {
    getConfig().then(setConfig).catch((err) => console.error(err));
  }, []);

  if (!config) {
    return <div className="p-4 text-sm text-gray-500">Loading config...</div>;
  }

  return (
    <div className="h-full overflow-y-auto p-5">
      <h2 className="text-sm font-semibold text-gray-100">Configuration</h2>
      <p className="mt-1 text-xs text-gray-500">{config.config_path}</p>
      <div className="mt-5 space-y-3">
        {Object.values(config.backends).map((backend) => (
          <div key={backend.backend} className="rounded-md border border-white/10 bg-white/[0.03] p-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-sm font-medium text-gray-200">{backend.backend}</div>
                <div className="text-xs text-gray-500">
                  {backend.model?.name ?? "No model"} · API key {backend.model?.api_key_configured ? "configured" : "missing"}
                </div>
              </div>
              <span className={backend.enabled ? "text-xs text-emerald-400" : "text-xs text-gray-500"}>
                {backend.enabled ? "Enabled" : "Disabled"}
              </span>
            </div>
            <div className="mt-3 flex gap-2">
              <input
                type="password"
                value={apiKeys[backend.backend] ?? ""}
                onChange={(event) => setApiKeys({ ...apiKeys, [backend.backend]: event.target.value })}
                placeholder="Update API key"
                className="flex-1 rounded-md border border-white/10 bg-black/20 px-3 py-2 text-xs text-gray-200 outline-none focus:border-primary"
              />
              <button
                className="rounded-md bg-primary px-3 py-2 text-xs text-white"
                onClick={async () => {
                  const updated = await saveBackendModel(backend.backend, {
                    api_key_configured: Boolean(backend.model?.api_key_configured),
                    api_key_update: apiKeys[backend.backend] ?? "",
                  });
                  setConfig(updated);
                  setApiKeys({ ...apiKeys, [backend.backend]: "" });
                }}
              >
                Save
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
