import { useEffect, useState } from "react";
import { NavLink, Outlet } from "react-router-dom";
import {
  LayoutDashboard,
  Network,
  KeyRound,
  ScrollText,
  Settings,
  Boxes,
  Monitor,
  Sun,
  Moon,
  BookOpen,
  Activity,
  CircleSlash,
  LoaderCircle,
  Server,
  Tag,
  ArrowUpCircle,
  CheckCircle2,
} from "lucide-react";
import { cn, formatListenUrl } from "@/lib/utils";
import { applyTheme, getStoredTheme, type ThemeMode } from "@/lib/theme";
import { serverApi } from "@/lib/api";
import type { ServerStatus } from "@/types";
import { CopyButton } from "@/components/ui/copy-button";
import { downloadPercent } from "@/lib/updater";
import { useUpdate } from "@/lib/update-store";

const NAV_ITEMS = [
  { to: "/", label: "仪表盘", icon: LayoutDashboard, end: true },
  { to: "/usage", label: "接入测试", icon: BookOpen },
  { to: "/services", label: "知识服务", icon: Boxes },
  { to: "/channels", label: "渠道管理", icon: Network },
  { to: "/api-keys", label: "密钥管理", icon: KeyRound },
  { to: "/logs", label: "请求日志", icon: ScrollText },
  { to: "/settings", label: "设置", icon: Settings },
];

const THEME_ICONS: Record<ThemeMode, typeof Sun> = {
  system: Monitor,
  light: Sun,
  dark: Moon,
};

function ThemeToggle() {
  // 用 state 跟踪当前主题：applyTheme 只改 localStorage/DOM，不会触发重渲染，
  // 否则高亮会永远停在首次渲染读到的值。
  const [current, setCurrent] = useState<ThemeMode>(() => getStoredTheme());
  const modes: ThemeMode[] = ["light", "dark", "system"];
  return (
    <div className="flex items-center gap-0.5 rounded-lg border bg-card p-0.5">
      {modes.map((m) => {
        const Icon = THEME_ICONS[m];
        const active = current === m;
        return (
          <button
            key={m}
            onClick={() => {
              applyTheme(m);
              setCurrent(m);
            }}
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

/**
 * 侧边栏底部的网关运行状态。
 *
 * 地址取自服务的运行态监听地址（而非设置里填的配置值），两者不一致时
 * 说明改动还没生效——这时以地址旁标注提示，避免误导。
 */
function GatewayStatus() {
  const [status, setStatus] = useState<ServerStatus | null>(null);
  // 更新状态来自全局 store（应用启动时已静默检查过一次），
  // 侧边栏只消费结果、不重复请求，也就不存在「两处状态打架」
  const {
    phase: updatePhase,
    info: updateInfo,
    progress: updateProgress,
    currentVersion,
    openDialog,
  } = useUpdate();

  useEffect(() => {
    let cancelled = false;

    const load = async () => {
      try {
        const s = await serverApi.status();
        if (!cancelled) setStatus(s);
      } catch (e) {
        console.error("Failed to load gateway status:", e);
      }
    };

    load();
    // 轮询：服务可能在设置页被停止/重启，侧边栏需跟随更新
    const timer = setInterval(load, 10_000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  const running = status?.running ?? false;
  // 配置与运行态不一致 → 展示的是旧地址，标注「待重启」避免误读
  const stale = running && (status?.restart_required ?? false);

  // 颜色只给图标，文字保持前景色：大字染色会像可点链接，且对比度不如前景色。
  // 「已停止」用灰色而非红色——主动停止属预期状态，红色留给真正的启动失败。
  const StatusIcon = !status ? LoaderCircle : running ? Activity : CircleSlash;
  const statusIconClass = !status
    ? "text-muted-foreground"
    : stale
      ? "text-warning"
      : running
        ? "text-success"
        : "text-muted-foreground";
  const statusText = !status ? "加载中" : running ? "运行中" : "已停止";
  const address =
    running && status
      ? formatListenUrl(status.host, status.port)
      : status
        ? String(status.configured_port)
        : "--";

  // none = 已是最新/还没查完 → 静态展示；有更新才让卡片可点
  const updateState: "none" | "available" | "downloading" | "installed" =
    updatePhase.kind === "downloading"
      ? "downloading"
      : updatePhase.kind === "installed"
        ? "installed"
        : updatePhase.kind === "available"
          ? "available"
          : "none";
  const displayVersion =
    updateState === "installed" && updateInfo
      ? updateInfo.version
      : currentVersion;
  const downloadingPct = downloadPercent(
    updateProgress?.downloaded ?? 0,
    updateProgress?.total,
  );

  // 卡片左半部分两种形态共用，只有右侧的状态指示不同
  const versionBody = (
    <>
      <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-background">
        <Tag className="h-3.5 w-3.5 text-muted-foreground" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="text-[11px] text-muted-foreground">应用版本</div>
        <div className="mt-0.5 font-mono text-xs">
          v{displayVersion || "—"}
        </div>
        {updateState === "downloading" && (
          <div className="mt-1.5 h-[3px] w-full overflow-hidden rounded-full bg-foreground/10">
            <div
              className="h-full bg-success transition-[width] duration-300"
              style={{ width: `${downloadingPct}%` }}
            />
          </div>
        )}
      </div>
    </>
  );

  return (
    <div className="space-y-2 p-2.5">
      {/* 服务状态卡：外卡承载状态，地址收进内嵌的圆角气泡 */}
      <div className="rounded-xl bg-muted/50 p-3.5">
        <div className="text-xs text-muted-foreground">服务状态</div>

        <div className="mt-1.5 flex items-center gap-2">
          <StatusIcon className={cn("h-4 w-4 shrink-0", statusIconClass)} />
          <span
            className={cn(
              "text-[17px] font-medium",
              status && !running && "text-muted-foreground",
            )}
          >
            {statusText}
          </span>
        </div>

        <div className="mt-3 flex items-center gap-2.5 rounded-lg bg-background p-2.5">
          <div
            className={cn(
              "flex h-7 w-7 shrink-0 items-center justify-center rounded-lg",
              running ? "bg-success/10" : "bg-muted",
            )}
          >
            <Server
              className={cn(
                "h-3.5 w-3.5",
                running ? "text-success" : "text-muted-foreground",
              )}
            />
          </div>
          <div className="min-w-0 flex-1">
            {/* 复制按钮挪到标题行右侧：把整行宽度让给地址，否则 24 字符的
                http://127.0.0.1:9842/v1 在 256px 侧边栏里必然被截断 */}
            <div className="flex items-center justify-between gap-1.5">
              <span className="text-[11px] text-muted-foreground">
                {running ? "API BaseUrl 地址" : "配置端口"}
              </span>
              {status && (
                <CopyButton value={address} label="地址" variant="ghost" />
              )}
            </div>
            {/* break-all 而非 truncate：host 可能被改成 192.168.x.x 或 IPv6，
                地址长度不可控——宁可折行也绝不让用户看到残缺 URL */}
            <div className="mt-0.5 font-mono text-[11px] break-all">
              {address}
            </div>
          </div>
        </div>

        {stale && (
          <div className="mt-2 text-[11px] text-warning">
            配置已变更，重启服务后生效
          </div>
        )}
      </div>

      {/* 版本卡：无更新时是静态信息（不可点）；发现新版本才亮图标并整卡可点开更新弹窗 */}
      {updateState === "none" ? (
        <div className="flex items-center gap-2.5 rounded-xl bg-muted/50 p-3">
          {versionBody}
        </div>
      ) : (
        <button
          type="button"
          onClick={openDialog}
          title={
            updateState === "available" && updateInfo
              ? `发现新版本 v${updateInfo.version}，点击更新`
              : "查看更新进度"
          }
          className="flex w-full items-center gap-2.5 rounded-xl bg-muted/50 p-3 text-left transition-colors hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-hidden"
        >
          {versionBody}
          {updateState === "available" && (
            <span className="relative flex h-[18px] w-[18px] shrink-0 items-center justify-center">
              {/* 呼吸环：与运行状态点的 ping 同一套视觉语言 */}
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-success/40" />
              <ArrowUpCircle className="relative h-[18px] w-[18px] text-success" />
            </span>
          )}
          {updateState === "downloading" && (
            <span className="shrink-0 text-[11px] text-success">
              {downloadingPct}%
            </span>
          )}
          {updateState === "installed" && (
            <span className="flex shrink-0 items-center gap-1 text-[11px] text-success">
              <CheckCircle2 className="h-3.5 w-3.5" />
              已更新
            </span>
          )}
        </button>
      )}
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
        <nav className="flex-1 space-y-2 overflow-y-auto p-3">
          {NAV_ITEMS.map((item) => {
            const Icon = item.icon;
            return (
              <NavLink
                key={item.to}
                to={item.to}
                end={item.end}
                className={({ isActive }) =>
                  cn(
                    "group flex items-center gap-3 rounded-xl px-3 py-2.5 transition-all duration-200",
                    isActive ? "bg-accent" : "hover:bg-accent/50",
                  )
                }
              >
                {({ isActive }) => (
                  <>
                    <span
                      className={cn(
                        "flex h-9 w-9 shrink-0 items-center justify-center rounded-lg transition-colors",
                        isActive
                          ? "bg-primary text-primary-foreground"
                          : "bg-muted text-muted-foreground group-hover:bg-accent group-hover:text-foreground",
                      )}
                    >
                      <Icon className="h-[18px] w-[18px]" />
                    </span>
                    <span
                      className={cn(
                        "text-[17px] transition-colors",
                        isActive
                          ? "font-medium text-foreground"
                          : "text-muted-foreground group-hover:text-foreground",
                      )}
                    >
                      {item.label}
                    </span>
                  </>
                )}
              </NavLink>
            );
          })}
        </nav>

        {/* Footer status */}
        <GatewayStatus />
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
