import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import { initTheme } from "./lib/theme";
import { setupTrayBehaviors } from "./lib/tray";
import { ToastProvider } from "./components/ui/toast";
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
        <App />
      </ToastProvider>
    </QueryClientProvider>
  </StrictMode>
);
