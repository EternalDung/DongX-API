import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import { initTheme } from "./lib/theme";
import { setupTrayBehaviors } from "./lib/tray";
import { ToastProvider } from "./components/ui/toast";
import { UpdateProvider } from "./lib/update-store";
import "./index.css";

initTheme();
// 最小化到托盘行为（关闭到托盘在 Rust 侧处理）
setupTrayBehaviors();
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
        {/* 更新状态必须全局单例：侧边栏版本卡与「关于」页共用同一份，
            否则两处各自 check() 会出现「一边说有新版本、一边说已是最新」 */}
        <UpdateProvider>
          <App />
        </UpdateProvider>
      </ToastProvider>
    </QueryClientProvider>
  </StrictMode>
);
