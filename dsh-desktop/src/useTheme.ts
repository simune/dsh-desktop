import { useEffect, useState } from "react";

/**
 * useTheme —— 标题栏主题跟随。
 *
 * dsh 网页默认主题策略为 `system`(见 dsh-client-ui-theme:
 * preference === "system" → matchMedia('(prefers-color-scheme: dark)'))。
 * 因此标题栏跟随系统 `prefers-color-scheme` 即可与 dsh 明暗切换同步。
 *
 * 同时监听壳页面自身 body[data-ds-dark-theme] 作为扩展点(若未来壳页面
 * 也启用 dsh token 主题)。注意:dsh 内容渲染在跨源 iframe 中,其内部
 * body 属性无法被壳页面读取,故以系统主题为准。
 */
export function useTheme(): { dark: boolean } {
  const [dark, setDark] = useState<boolean>(() => {
    try {
      return window.matchMedia("(prefers-color-scheme: dark)").matches;
    } catch {
      return false;
    }
  });

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onMq = (e: MediaQueryListEvent) => setDark(e.matches);
    mq.addEventListener?.("change", onMq);

    // 壳页面自身 body 主题属性(扩展点;当前壳页面未启用 dsh token 主题)
    const body = document.body;
    const onBody = () => {
      if (body.hasAttribute("data-ds-dark-theme")) setDark(true);
      else setDark(mq.matches);
    };
    const mo = new MutationObserver(onBody);
    mo.observe(body, { attributes: true, attributeFilter: ["data-ds-dark-theme"] });

    return () => {
      mq.removeEventListener?.("change", onMq);
      mo.disconnect();
    };
  }, []);

  return { dark };
}
