import { useEffect, useRef } from "react";

/**
 * 键盘 Tab 切换页签——复用于服务页 / RAG 详情 / Wiki 详情等任意受控 Tabs。
 *
 * 行为（与服务页一致）：
 * - Tab = 下一个页签，Shift+Tab = 上一个页签，到达末尾循环回开头。
 * - 焦点在 INPUT / TEXTAREA / contentEditable 时不劫持，保留浏览器正常的焦点移动。
 * - 传入的 ids 即「可切换页签的顺序」；若某页签禁用，请调用方自行剔除（如 Wiki 的图谱）。
 *
 * 实现说明：监听器只在挂载时绑定一次，最新 activeTab / ids / setter 通过 ref 读取，
 * 避免每次页签变化都解绑重绑，也避免把 ids 数组写进依赖导致频繁重绑。
 */
export function useTabKeyNavigation(
  ids: string[],
  activeTab: string,
  setActiveTab: (value: string) => void,
  enabled = true,
): void {
  const idsRef = useRef(ids);
  const activeRef = useRef(activeTab);
  const setRef = useRef(setActiveTab);

  // 每次渲染同步最新值；监听器只绑定一次，靠 ref 读取最新状态。
  idsRef.current = ids;
  activeRef.current = activeTab;
  setRef.current = setActiveTab;

  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const el = e.target as HTMLElement | null;
      const tag = el?.tagName;
      // 输入框 / 文本域 / 可编辑区内保留浏览器默认的 Tab 焦点移动。
      if (tag === "INPUT" || tag === "TEXTAREA" || el?.isContentEditable) return;
      e.preventDefault();
      const list = idsRef.current;
      if (list.length === 0) return;
      const idx = list.indexOf(activeRef.current);
      const start = idx < 0 ? 0 : idx;
      const delta = e.shiftKey ? -1 : 1;
      const next = list[(start + delta + list.length) % list.length];
      setRef.current(next);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [enabled]);
}
