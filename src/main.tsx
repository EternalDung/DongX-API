import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import { initTheme } from "./lib/theme";
import { setupTrayBehaviors } from "./lib/tray";
import { ToastProvider } from "./components/ui/toast";
import { startupAutoCheck } from "./lib/updater";
import "./index.css";

initTheme();
// 最小化到托盘行为（关闭到托盘在 Rust 侧处理）
setupTrayBehaviors();
// 启动后静默检查更新：发现新版本则发系统通知，不弹窗不打扰。
// 失败一律静默（网络/插件未就绪都不影响正常使用）。
void startupAutoCheck();

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 1000 * 60, // 1 minute
      retry: 1,
    },
  },
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <App />
      </ToastProvider>
    </QueryClientProvider>
  </StrictMode>
);
