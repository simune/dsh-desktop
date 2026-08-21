import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

/**
 * useTheme —— 标题栏/加载页主题跟随 dsh 网页内的主题设置。
 *
 * dsh 的主题偏好存于 `$DSH_HOME/settings.yaml` 的 `ui-theme.preference`
 * (light | dark | system,默认 system)。用户可在 dsh 内手动切换深色/浅色,
 * 该值只对 dsh iframe 生效,壳页面(跨源)读不到 iframe 的 DOM。
 *
 * 因此这里通过 Rust 侧读取该偏好:
 * - 启动时 invoke get_theme_preference 取当前值
 * - Rust 后台轮询 settings.yaml,变化时 emit theme-changed
 * - preference 优先:dark→深色,light→浅色,system/缺失→跟随 prefers-color-scheme
 *
 * 结果写到 <html data-theme="dark|light">,由 App.css 的属性选择器驱动全部壳页面配色。
 */
export function useTheme(): { dark: boolean } {
  const [dark, setDark] = useState<boolean>(false);

  useEffect(() => {
    let cancelled = false;
    const systemDark = () => {
      try {
        return window.matchMedia("(prefers-color-scheme: dark)").matches;
      } catch {
        return false;
      }
    };
    const apply = (pref: string) => {
      if (cancelled) return;
      if (pref === "dark") setDark(true);
      else if (pref === "light") setDark(false);
      else setDark(systemDark());
    };

    invoke<string>("get_theme_preference")
      .then(apply)
      .catch(() => apply("system"));

    let unlisten: (() => void) | undefined;
    listen<string>("theme-changed", (e) => apply(e.payload))
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // 写 <html data-theme>,驱动 App.css 属性选择器
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
  }, [dark]);

  return { dark };
}
