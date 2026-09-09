// 设置页里的「自动更新」卡片：手动检查更新、显示版本/更新说明、下载进度条、
// 安装完成后由 updater.ts 自动 relaunch（mac/linux 必需；windows 安装器自启）。
import { useEffect, useState } from "react";
import { Download, RefreshCw, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { useToast } from "@/components/ui/toast";
import { cn } from "@/lib/utils";
import {
  checkForUpdate,
  downloadAndInstall,
  getCurrentVersion,
  type DownloadProgress,
  type UpdateInfo,
} from "@/lib/updater";

type Phase =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "uptodate"; current: string }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "downloading"; info: UpdateInfo; progress: DownloadProgress }
  | { kind: "error"; current: string; message: string };

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function pct(downloaded: number, total: number): number {
  if (total <= 0) return 0;
  return Math.min(100, Math.round((downloaded / total) * 100));
}

export function UpdateSection() {
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [current, setCurrent] = useState<string>("…");
  const toast = useToast();

  // 启动时取一次当前版本（来自 tauri.conf.json 的 version 字段）
  useEffect(() => {
    getCurrentVersion()
      .then(setCurrent)
      .catch(() => setCurrent("未知"));
  }, []);

  const handleCheck = async () => {
    setPhase({ kind: "checking" });
    try {
      const info = await checkForUpdate();
      if (!info) {
        setPhase({ kind: "uptodate", current });
      } else {
        setPhase({ kind: "available", info });
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setPhase({ kind: "error", current, message: msg });
      toast.error(`检查更新失败：${msg}`);
    }
  };

  const handleDownload = async (info: UpdateInfo) => {
    setPhase({
      kind: "downloading",
      info,
      progress: { event: "Started", downloaded: 0, total: undefined },
    });
    try {
      await downloadAndInstall((p) => {
        setPhase({ kind: "downloading", info, progress: p });
      });
      // mac/linux 上 downloadAndInstall 内部已 relaunch() 退出当前进程，
      // 这行多数情况下不会执行；windows 上 NSIS 安装器会自启，这里也基本到不了。
      // 留作防御：如果走到这里，说明环境特殊，提示用户手动重启。
      toast.info("安装完成，请手动重启应用以应用新版本。");
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(`下载失败：${msg}`);
      setPhase({ kind: "available", info });
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Sparkles className="h-4 w-4" /> 自动更新
        </CardTitle>
        <CardDescription>
          当前版本 v{current}。从 GitHub Releases 拉取签名校验过的最新版本，一键安装。
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        {(phase.kind === "idle" ||
          phase.kind === "checking" ||
          phase.kind === "uptodate" ||
          phase.kind === "error") && (
          <div className="flex items-center justify-between gap-4">
            <div className="text-sm text-muted-foreground">
              {phase.kind === "idle" && "点击右侧按钮检查新版本"}
              {phase.kind === "checking" && "正在向 GitHub Releases 查询…"}
              {phase.kind === "uptodate" && "已是最新版本 ✓"}
              {phase.kind === "error" && (
                <span className="text-destructive">检查失败：{phase.message}</span>
              )}
            </div>
            <Button
              onClick={handleCheck}
              disabled={phase.kind === "checking"}
              variant="outline"
            >
              <RefreshCw
                className={cn(
                  "mr-1 h-4 w-4",
                  phase.kind === "checking" && "animate-spin",
                )}
              />
              {phase.kind === "checking" ? "检查中…" : "检查更新"}
            </Button>
          </div>
        )}

        {(phase.kind === "available" ||
          phase.kind === "downloading") && (
          <div className="grid gap-3">
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant="secondary">v{phase.info.version}</Badge>
              <span className="text-sm text-muted-foreground">
                v{phase.info.currentVersion} → v{phase.info.version}
              </span>
              {phase.info.date && (
                <span className="text-xs text-muted-foreground">
                  · {phase.info.date.slice(0, 10)}
                </span>
              )}
            </div>

            {phase.info.body && phase.info.body.trim() !== "" && (
              <pre className="max-h-40 overflow-auto rounded-md border border-border/60 bg-muted/30 p-3 text-xs whitespace-pre-wrap text-muted-foreground">
                {phase.info.body}
              </pre>
            )}

            {phase.kind === "available" && (
              <div className="flex justify-end">
                <Button onClick={() => handleDownload(phase.info)}>
                  <Download className="mr-1 h-4 w-4" />
                  下载并安装
                </Button>
              </div>
            )}

            {phase.kind === "downloading" && (
              <div className="grid gap-2">
                <div className="text-xs text-muted-foreground">
                  {phase.progress.event === "Started" && "准备下载…"}
                  {phase.progress.event === "Progress" &&
                    (phase.progress.total
                      ? `${formatBytes(phase.progress.downloaded)} / ${formatBytes(phase.progress.total)}（${pct(phase.progress.downloaded, phase.progress.total)}%）`
                      : `${formatBytes(phase.progress.downloaded)}`)}
                  {phase.progress.event === "Finished" &&
                    "下载完成，正在安装并重启应用…"}
                </div>
                {phase.progress.total !== undefined &&
                  phase.progress.total > 0 && (
                    <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
                      <div
                        className="h-full bg-primary transition-[width] duration-300"
                        style={{
                          width: `${pct(phase.progress.downloaded, phase.progress.total)}%`,
                        }}
                      />
                    </div>
                  )}
              </div>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}