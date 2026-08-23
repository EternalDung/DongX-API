import { NavLink, Outlet } from "react-router-dom";
import {
  LayoutDashboard,
  Network,
  KeyRound,
  ScrollText,
  ShieldAlert,
  Settings,
  Boxes,
  Monitor,
  Sun,
  Moon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { applyTheme, getStoredTheme, type ThemeMode } from "@/lib/theme";

const NAV_ITEMS = [
  { to: "/", label: "仪表盘", icon: LayoutDashboard, end: true },
  { to: "/channels", label: "渠道管理", icon: Network },
  { to: "/api-keys", label: "密钥管理", icon: KeyRound },
  { to: "/logs", label: "请求日志", icon: ScrollText },
  { to: "/audit", label: "安全审计", icon: ShieldAlert },
  { to: "/settings", label: "设置", icon: Settings },
];

const THEME_ICONS: Record<ThemeMode, typeof Sun> = {
  system: Monitor,
  light: Sun,
  dark: Moon,
};

function ThemeToggle() {
  const current = getStoredTheme();
  const modes: ThemeMode[] = ["light", "dark", "system"];
  return (
    <div className="flex items-center gap-0.5 rounded-lg border bg-card p-0.5">
      {modes.map((m) => {
        const Icon = THEME_ICONS[m];
        const active = current === m;
        return (
          <button
            key={m}
            onClick={() => applyTheme(m)}
            title={`主题：${m}`}
            className={cn(
              "flex h-7 w-7 items-center justify-center rounded-md transition-colors",
              active
                ? "bg-primary text-primary-foreground"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            <Icon className="h-3.5 w-3.5" />
          </button>
        );
      })}
    </div>
  );
}

export default function Layout() {
  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      {/* Sidebar */}
      <aside className="flex w-64 shrink-0 flex-col border-r bg-card/50 backdrop-blur-sm">
        {/* Brand */}
        <div className="flex h-16 items-center gap-3 border-b px-5">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-primary text-primary-foreground shadow-sm shadow-primary/30">
            <Boxes className="h-5 w-5" />
          </div>
          <div className="leading-tight">
            <div className="text-base font-semibold tracking-tight">DongX</div>
            <div className="text-[11px] text-muted-foreground">本地 LLM API 网关</div>
          </div>
        </div>

        {/* Nav */}
        <nav className="flex-1 space-y-1 overflow-y-auto p-3">
          {NAV_ITEMS.map((item) => {
            const Icon = item.icon;
            return (
              <NavLink
                key={item.to}
                to={item.to}
                end={item.end}
                className={({ isActive }) =>
                  cn(
                    "group relative flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm transition-all duration-200",
                    isActive
                      ? "bg-accent font-medium text-accent-foreground"
                      : "text-muted-foreground hover:bg-accent/50 hover:text-foreground",
                  )
                }
              >
                {({ isActive }) => (
                  <>
                    {isActive && (
                      <span className="absolute left-0 top-1/2 h-5 w-1 -translate-y-1/2 rounded-r-full bg-primary" />
                    )}
                    <Icon
                      className={cn(
                        "h-4 w-4 shrink-0 transition-colors",
                        isActive ? "text-primary" : "text-muted-foreground group-hover:text-foreground",
                      )}
                    />
                    {item.label}
                  </>
                )}
              </NavLink>
            );
          })}
        </nav>

        {/* Footer status */}
        <div className="border-t p-4">
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <span className="relative flex h-2 w-2">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-success/60" />
              <span className="relative inline-flex h-2 w-2 rounded-full bg-success" />
            </span>
            网关运行中
          </div>
          <div className="mt-1 font-mono text-[11px] text-muted-foreground">
            localhost:9842
          </div>
        </div>
      </aside>

      {/* Main column */}
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center justify-between border-b bg-background/80 px-8 backdrop-blur">
          <div className="text-sm text-muted-foreground">
            OpenAI 兼容 · 多供应商统一接入
          </div>
          <ThemeToggle />
        </header>

        <main className="flex-1 overflow-auto">
          <div className="mx-auto w-full max-w-[1240px] px-8 py-8">
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  );
}
