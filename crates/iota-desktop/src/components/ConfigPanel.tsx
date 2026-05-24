import { useEffect, useState } from "react";
import { getConfig, saveBackendModel } from "../api";
import type { DesktopConfigSnapshot, DesktopModelConfig } from "../types";

type Props = {
  config?: DesktopConfigSnapshot | null;
  onConfigUpdate?: (config: DesktopConfigSnapshot) => void;
};

export function ConfigPanel({ config: externalConfig, onConfigUpdate }: Props) {
  const [config, setConfig] = useState<DesktopConfigSnapshot | null>(null);
  const [edits, setEdits] = useState<Record<string, Partial<DesktopModelConfig>>>({});

  useEffect(() => {
    if (externalConfig) {
      setConfig(externalConfig);
      return;
    }
    getConfig()
      .then((snapshot) => {
        setConfig(snapshot);
        onConfigUpdate?.(snapshot);
      })
      .catch((err) => console.error(err));
  }, [externalConfig, onConfigUpdate]);

  if (!config) {
    return <div className="p-4 text-sm text-gray-500">Loading config...</div>;
  }

  return (
    <div className="h-full overflow-y-auto p-5">
      <h2 className="text-sm font-semibold text-gray-100">Configuration</h2>
      <p className="mt-1 text-xs text-gray-500">{config.config_path}</p>
      <div className="mt-5 space-y-3">
        {Object.values(config.backends).map((backend) => {
          const draft = edits[backend.backend] ?? {};
          const model = backend.model;
          const updateDraft = (patch: Partial<DesktopModelConfig>) =>
            setEdits({ ...edits, [backend.backend]: { ...draft, ...patch } });

          return (
            <div key={backend.backend} className="rounded-md border border-white/10 bg-white/[0.03] p-4">
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-sm font-medium text-gray-200">{backend.backend}</div>
                  <div className="text-xs text-gray-500">
                    {backend.model?.name ?? "No model"} · API key{" "}
                    {backend.model?.api_key_configured ? "configured" : "missing"}
                  </div>
                </div>
                <span className={backend.enabled ? "text-xs text-emerald-400" : "text-xs text-gray-500"}>
                  {backend.enabled ? "Enabled" : "Disabled"}
                </span>
              </div>
              <div className="mt-3 grid grid-cols-1 gap-2 md:grid-cols-2">
                <input
                  value={draft.provider ?? model?.provider ?? ""}
                  onChange={(event) => updateDraft({ provider: event.target.value })}
                  placeholder="Provider"
                  className="rounded-md border border-white/10 bg-black/20 px-3 py-2 text-xs text-gray-200 outline-none focus:border-primary"
                />
                <input
                  value={draft.name ?? model?.name ?? ""}
                  onChange={(event) => updateDraft({ name: event.target.value })}
                  placeholder="Model name"
                  className="rounded-md border border-white/10 bg-black/20 px-3 py-2 text-xs text-gray-200 outline-none focus:border-primary"
                />
                <input
                  value={draft.base_url ?? model?.base_url ?? ""}
                  onChange={(event) => updateDraft({ base_url: event.target.value })}
                  placeholder="Base URL"
                  className="rounded-md border border-white/10 bg-black/20 px-3 py-2 text-xs text-gray-200 outline-none focus:border-primary md:col-span-2"
                />
                <input
                  type="password"
                  value={draft.api_key_update ?? ""}
                  onChange={(event) => updateDraft({ api_key_update: event.target.value })}
                  placeholder="Update API key"
                  className="rounded-md border border-white/10 bg-black/20 px-3 py-2 text-xs text-gray-200 outline-none focus:border-primary"
                />
                <button
                  className="rounded-md bg-primary px-3 py-2 text-xs text-white"
                  onClick={async () => {
                    const apiKey = draft.api_key_update?.trim();
                    const updated = await saveBackendModel(backend.backend, {
                      api_key_configured: Boolean(backend.model?.api_key_configured),
                      ...(draft.provider !== undefined ? { provider: draft.provider } : {}),
                      ...(draft.name !== undefined ? { name: draft.name } : {}),
                      ...(draft.base_url !== undefined ? { base_url: draft.base_url } : {}),
                      ...(apiKey ? { api_key_update: apiKey } : {}),
                    });
                    setConfig(updated);
                    onConfigUpdate?.(updated);
                    setEdits({ ...edits, [backend.backend]: {} });
                  }}
                >
                  Save
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
