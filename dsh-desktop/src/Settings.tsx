import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type PortPolicy = { mode: "auto" } | { mode: "fixed"; port: number };
type AppSettings = {
  dsh_home: string | null;
  port_policy: PortPolicy;
  cwd: string | null;
  autostart: boolean;
  log_lines: number;
};

export default function SettingsView() {
  const [s, setS] = useState<AppSettings | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    invoke<AppSettings>("get_settings").then(setS).catch(console.error);
    refreshLogs();
  }, []);

  async function refreshLogs() {
    setLogs(await invoke<string[]>("get_logs", { tail: 100 }));
  }

  async function save() {
    if (!s) return;
    try {
      await invoke("set_settings", { new: s });
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
    } catch (e) {
      console.error(e);
      alert(String(e));
    }
  }

  if (!s) return <div className="container">加载设置…</div>;

  return (
    <main className="settings">
      <h1>设置</h1>

      <label>
        DSH_HOME(留空 = 跟随环境变量/默认 ~/.dsh)
        <input
          value={s.dsh_home ?? ""}
          placeholder="~/.dsh"
          onChange={(e) => setS({ ...s, dsh_home: e.target.value || null })}
        />
      </label>

      <fieldset>
        <legend>端口策略(下次重启服务生效)</legend>
        <label className="row">
          <input
            type="radio"
            checked={s.port_policy.mode === "auto"}
            onChange={() => setS({ ...s, port_policy: { mode: "auto" } })}
          />
          自动(--port 0,推荐)
        </label>
        <label className="row">
          <input
            type="radio"
            checked={s.port_policy.mode === "fixed"}
            onChange={() => setS({ ...s, port_policy: { mode: "fixed", port: 3080 } })}
          />
          固定端口
          {s.port_policy.mode === "fixed" && (
            <input
              type="number"
              value={s.port_policy.port}
              min={1}
              max={65535}
              onChange={(e) =>
                setS({ ...s, port_policy: { mode: "fixed", port: Number(e.target.value) } })
              }
            />
          )}
        </label>
      </fieldset>

      <label>
        日志缓冲行数
        <input
          type="number"
          value={s.log_lines}
          min={100}
          step={100}
          onChange={(e) => setS({ ...s, log_lines: Number(e.target.value) })}
        />
      </label>

      <label className="row">
        <input
          type="checkbox"
          checked={s.autostart}
          onChange={(e) => setS({ ...s, autostart: e.target.checked })}
        />
        开机自启(登录时启动)
      </label>

      <div className="actions">
        <button onClick={save}>{saved ? "已保存 ✓" : "保存"}</button>
        <button className="secondary" onClick={() => invoke("quit_app")}>
          退出
        </button>
      </div>

      <section className="logs-section">
        <h2>服务日志</h2>
        <button className="secondary" onClick={refreshLogs}>
          刷新日志
        </button>
        <pre className="logs">
          {logs.length ? logs.join("\n") : "(暂无日志)"}
        </pre>
      </section>
    </main>
  );
}
