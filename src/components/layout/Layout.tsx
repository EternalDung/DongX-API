import { NavLink, Outlet } from "react-router-dom";
import {
  LayoutDashboard,
  Network,
  KeyRound,
  ScrollText,
  ShieldAlert,
  Settings,
} from "lucide-react";
import { cn } from "@/lib/utils";

const NAV_ITEMS = [
  { to: "/", label: "仪表盘", icon: LayoutDashboard, end: true },
  { to: "/channels", label: "渠道管理", icon: Network },
  { to: "/api-keys", label: "密钥管理", icon: KeyRound },
  { to: "/logs", label: "请求日志", icon: ScrollText },
  { to: "/audit", label: "安全审计", icon: ShieldAlert },
  { to: "/settings", label: "设置", icon: Settings },
];

export default function Layout() {
  return (
    <div className="flex h-screen w-screen overflow-hidden">
      {/* Sidebar */}
      <aside className="flex w-60 flex-col border-r bg-card">
        <div className="flex h-14 items-center gap-2 border-b px-6">
          <span className="text-lg font-bold">DongX</span>
          <span className="text-xs text-muted-foreground">v0.1.0</span>
        </div>
        <nav className="flex-1 space-y-1 p-3">
          {NAV_ITEMS.map((item) => {
            const Icon = item.icon;
            return (
              <NavLink
                key={item.to}
                to={item.to}
                end={item.end}
                className={({ isActive }) =>
                  cn(
                    "flex items-center gap-3 rounded-md px-3 py-2 text-sm transition-colors",
                    isActive
                      ? "bg-accent text-accent-foreground font-medium"
                      : "text-muted-foreground hover:bg-accent/50 hover:text-foreground"
                  )
                }
              >
                <Icon className="h-4 w-4" />
                {item.label}
              </NavLink>
            );
          })}
        </nav>
        <div className="border-t p-4">
          <p className="text-xs text-muted-foreground">
            本地 LLM API 网关
          </p>
          <p className="text-xs text-muted-foreground">localhost:9842</p>
        </div>
      </aside>

      {/* Main content */}
      <main className="flex-1 overflow-auto">
        <Outlet />
      </main>
    </div>
  );
}
