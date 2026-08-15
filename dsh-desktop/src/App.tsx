import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

/**
 * 壳 UI(M1):加载页 / 错误页,由 server-status 事件驱动。
 * running → 导航到 dsh UI;error → 展示错误与日志;retry → restart_server。
 */
type ServerStatus =
  | { state: "starting" }
  | { state: "running"; url: string }
  | { state: "stopping" }
  | { state: "stopped" }
  | { state: "error"; code: string; message: string };

function App() {
  const [status, setStatus] = useState<ServerStatus | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [ipcError, setIpcError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    function apply(s: ServerStatus) {
      if (cancelled) return;
      setStatus(s);
      if (s.state === "running" && s.url) {
        window.location.href = s.url; // 离开本地壳 → dsh UI
      }
    }

    const unlisten = listen<ServerStatus>("server-status", (e) => apply(e.payload));

    async function poll() {
      while (!cancelled) {
        try {
          const s = await invoke<ServerStatus>("get_server_status");
          setIpcError(null);
          apply(s);
          if (s.state === "running") return;
        } catch (e) {
          setIpcError(String(e));
        }
        await new Promise((r) => setTimeout(r, 300));
      }
    }
    poll();

    return () => {
      cancelled = true;
      unlisten.then((f) => f());
    };
  }, []);

  async function retry() {
    setBusy(true);
    try {
      await invoke("restart_server");
    } catch (e) {
      console.error(e);
    } finally {
      setBusy(false);
    }
  }

  async function showLogs() {
    setLogs(await invoke<string[]>("get_logs", { tail: 200 }));
  }

  const isError = status?.state === "error";
  const isStopped = status?.state === "stopped";

  return (
    <main className="container">
      {isError ? (
        <>
          <h1 className="err">服务启动失败</h1>
          <p className="code">
            {status.state === "error" ? status.code : ""} —{" "}
            {status.state === "error" ? status.message : ""}
          </p>
          <div className="actions">
            <button onClick={retry} disabled={busy}>
              {busy ? "重启中…" : "重试"}
            </button>
            <button onClick={showLogs}>查看日志</button>
            <button onClick={() => invoke("quit_app")}>退出</button>
          </div>
          {logs.length > 0 && (
            <pre className="logs">
              {logs.join("\n")}
            </pre>
          )}
        </>
      ) : isStopped ? (
        <>
          <h1>服务已停止</h1>
          <div className="actions">
            <button onClick={retry} disabled={busy}>
              {busy ? "启动中…" : "启动服务"}
            </button>
            <button onClick={() => invoke("quit_app")}>退出</button>
          </div>
        </>
      ) : (
        <>
          <h1>DSH Desktop</h1>
          <p>正在启动 dsh 服务…</p>
          {ipcError && <p className="code">IPC 错误: {ipcError}</p>}
          <div className="spinner" />
          <p className="dsh-url">
            {status?.state === "running" && "running"}
            {status?.state === "starting" && "starting"}
            {status?.state === "stopping" && "stopping"}
          </p>
        </>
      )}
    </main>
  );
}

export default App;
