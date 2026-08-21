import { useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTheme } from "./useTheme";

/**
 * TitleBar —— dsh 风格自绘标题栏(无边框窗口用)。
 *
 * - 左侧:应用图标 + 标题;中间:data-tauri-drag-region 拖拽区;右侧:三键。
 * - 三键调用 @tauri-apps/api/window(权限已在 core:default,见 capabilities)。
 * - 主题跟随 useTheme()(dsh 默认跟随系统主题 prefers-color-scheme)。
 * - 主窗口关闭仍走 Rust CloseRequested → request_exit(停服务+退出),语义不变。
 */
export default function TitleBar({ title = "DSH Desktop" }: { title?: string }) {
  const { dark } = useTheme();
  const [maximized, setMaximized] = useState(false);
  const win = getCurrentWindow();

  const theme = dark ? "dark" : "light";

  async function toggleMaximize() {
    const isMax = await win.isMaximized();
    if (isMax) await win.unmaximize();
    else await win.maximize();
    setMaximized(!isMax);
  }

  return (
    <header
      className={`titlebar tb-${theme}`}
      data-tauri-drag-region
      data-dsh-titlebar
    >
      <div className="tb-left" data-tauri-drag-region>
        <svg
          className="tb-icon"
          data-tauri-drag-region
          width="16"
          height="16"
          viewBox="0 0 16 16"
          fill="none"
          aria-hidden
        >
          <path
            d="M8 1.2c.55 0 1.06.23 1.42.6l4.78 4.78a2 2 0 0 1 0 2.84l-4.78 4.78a2 2 0 0 1-2.84 0L1.8 9.42a2 2 0 0 1 0-2.84L6.58 1.8A2 2 0 0 1 8 1.2Zm0 1.4a.6.6 0 0 0-.42.18L2.78 8.58a.6.6 0 0 0 0 .84l4.8 4.8a.6.6 0 0 0 .84 0l4.8-4.8a.6.6 0 0 0 0-.84L8.42 2.78A.6.6 0 0 0 8 2.6Z"
            fill="currentColor"
          />
        </svg>
        <span className="tb-title" data-tauri-drag-region>
          {title}
        </span>
      </div>

      <div className="tb-drag" data-tauri-drag-region />

      <div className="tb-controls" data-tauri-drag-region="false">
        <button
          className="tb-btn"
          aria-label="最小化"
          title="最小化"
          onClick={() => win.minimize()}
        >
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
            <path d="M0 5h10v1H0z" fill="currentColor" />
          </svg>
        </button>
        <button
          className="tb-btn"
          aria-label={maximized ? "还原" : "最大化"}
          title={maximized ? "还原" : "最大化"}
          onClick={toggleMaximize}
        >
          {maximized ? (
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
              <path
                d="M2.5 1.5h6v6h-6zM1.5 3h.5v6h6v.5H1.5z"
                fill="currentColor"
              />
            </svg>
          ) : (
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
              <path
                d="M2.5 2.5v5h5v-5h-5ZM1.5 1.5h7v7h-7z"
                fill="currentColor"
              />
            </svg>
          )}
        </button>
        <button
          className="tb-btn tb-close"
          aria-label="关闭"
          title="关闭"
          onClick={() => win.close()}
        >
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
            <path
              d="M1.2 1.2l7.6 7.6m0-7.6L1.2 8.8"
              stroke="currentColor"
              strokeWidth="1.2"
            />
          </svg>
        </button>
      </div>
    </header>
  );
}
