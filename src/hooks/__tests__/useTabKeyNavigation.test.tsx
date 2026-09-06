import { describe, it, expect } from "vitest";
import { useState } from "react";
import { render, fireEvent, act, screen } from "@testing-library/react";
import { useTabKeyNavigation } from "../useTabKeyNavigation";

/**
 * 测试组件：用真实 state 驱动 activeTab，渲染当前值便于断言；
 * 同时挂一个 input 用于「焦点在输入框时不劫持 Tab」用例。
 */
function Harness({
  ids,
  initial,
  enabled,
}: {
  ids: string[];
  initial: string;
  enabled?: boolean;
}) {
  const [tab, setTab] = useState(initial);
  useTabKeyNavigation(ids, tab, setTab, enabled);
  return (
    <div>
      <span data-testid="tab">{tab}</span>
      <input data-testid="input" />
    </div>
  );
}

/** 在 window 上派发原生 Tab 键事件（hook 监听的是 window）。 */
function pressTab(shift = false) {
  act(() => {
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Tab", shiftKey: shift, bubbles: true }),
    );
  });
}

describe("useTabKeyNavigation", () => {
  it("Tab 切到下一个页签", () => {
    render(<Harness ids={["a", "b", "c"]} initial="a" />);
    pressTab();
    expect(screen.getByTestId("tab").textContent).toBe("b");
  });

  it("Shift+Tab 切到上一个页签", () => {
    render(<Harness ids={["a", "b", "c"]} initial="b" />);
    pressTab(true);
    expect(screen.getByTestId("tab").textContent).toBe("a");
  });

  it("末尾 Tab 循环回开头", () => {
    render(<Harness ids={["a", "b", "c"]} initial="c" />);
    pressTab();
    expect(screen.getByTestId("tab").textContent).toBe("a");
  });

  it("开头 Shift+Tab 循环到末尾", () => {
    render(<Harness ids={["a", "b", "c"]} initial="a" />);
    pressTab(true);
    expect(screen.getByTestId("tab").textContent).toBe("c");
  });

  it("连续多次 Tab 依次前进", () => {
    render(<Harness ids={["a", "b", "c"]} initial="a" />);
    pressTab(); // a -> b
    pressTab(); // b -> c
    expect(screen.getByTestId("tab").textContent).toBe("c");
  });

  it("ids 为空时不做任何切换", () => {
    render(<Harness ids={[]} initial="a" />);
    pressTab();
    expect(screen.getByTestId("tab").textContent).toBe("a");
  });

  it("active 不在 ids 中时从下标 0 起算", () => {
    render(<Harness ids={["a", "b", "c"]} initial="x" />);
    pressTab();
    expect(screen.getByTestId("tab").textContent).toBe("b");
  });

  it("焦点在 input 时不劫持 Tab", () => {
    render(<Harness ids={["a", "b", "c"]} initial="a" />);
    const input = screen.getByTestId("input") as HTMLInputElement;
    input.focus();
    act(() => {
      fireEvent.keyDown(input, { key: "Tab" });
    });
    expect(screen.getByTestId("tab").textContent).toBe("a");
  });

  it("enabled=false 时不绑定监听", () => {
    render(<Harness ids={["a", "b", "c"]} initial="a" enabled={false} />);
    pressTab();
    expect(screen.getByTestId("tab").textContent).toBe("a");
  });
});
