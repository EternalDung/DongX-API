// 更新弹窗（全局单例，由 UpdateProvider 渲染）。
//
// 纯展示组件：所有状态与动作都由 update-store 传入，这里不做任何 check/下载调用，
// 保证「侧边栏版本卡」和「关于」页点开的是同一个弹窗、同一份进度。
import type { ReactNode } from "react";
import { ArrowRight, Download, RefreshCw, RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { downloadPercent, formatBytes } from "@/lib/updater";
import type { UpdatePhase } from "@/lib/update-store";

interface UpdateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  phase: UpdatePhase;
  currentVersion: string;
  onCheck: () => void;
  onDownload: () => void;
  onRelaunch: () => void;
}

/** 统一的小标题行：当前版本 → 新版本 → 发布日期 */
function VersionRow({
  from,
  to,
  date,
}: {
  from: string;
  to: string;
  date?: string;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="rounded-md bg-muted px-2 py-0.5 font-mono text-xs text-muted-foreground">
        v{from}
      </span>
      <ArrowRight className="h-3.5 w-3.5 text-muted-foreground" />
      <span className="rounded-md bg-success/10 px-2 py-0.5 font-mono text-xs text-success">
        v{to}
      </span>
      {date && (
        <span className="text-[11px] text-muted-foreground">
          {date.slice(0, 10)}
        </span>
      )}
    </div>
  );
}

/** 把 `**加粗**` / 反引号代码 拆成对应行内元素，其余原样输出。 */
function renderInline(text: string, keyPrefix: string): ReactNode[] {
  return text
    .split(/(\*\*[^*]+\*\*|`[^`]+`)/g)
    .filter((part) => part !== "")
    .map((part, i) => {
      if (part.startsWith("**") && part.endsWith("**")) {
        return (
          <strong
            key={`${keyPrefix}-${i}`}
            className="font-medium text-foreground"
          >
            {part.slice(2, -2)}
          </strong>
        );
      }
      if (part.startsWith("`") && part.endsWith("`")) {
        return (
          <code
            key={`${keyPrefix}-${i}`}
            className="rounded bg-muted px-1 py-0.5 font-mono text-[11px]"
          >
            {part.slice(1, -1)}
          </code>
        );
      }
      return <span key={`${keyPrefix}-${i}`}>{part}</span>;
    });
}

/** 更新说明：内容来自 release notes（GitHub Release 正文 = latest.json 的 notes）。
 *  只认 ### 标题 / - 列表 / **加粗** / 反引号代码 / --- 分隔线这几种语法 ——
 *  这段文本由 .github/scripts/gen-release-notes.sh 生成，语法可控；
 *  为一段日志引入 react-markdown 不划算，所以这里手写一个极简渲染。 */
function ReleaseNotes({ body }: { body?: string }) {
  if (!body || body.trim() === "") return null;

  const blocks: ReactNode[] = [];
  let items: string[] = [];
  let seq = 0;

  const flushItems = () => {
    if (items.length === 0) return;
    const current = items;
    items = [];
    blocks.push(
      <ul key={`ul-${seq++}`} className="list-disc space-y-1 pl-4">
        {current.map((it, i) => (
          <li key={i}>{renderInline(it, `li-${i}`)}</li>
        ))}
      </ul>,
    );
  };

  for (const rawLine of body.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line === "") continue;

    if (/^-{3,}$/.test(line)) {
      flushItems();
      blocks.push(<hr key={`hr-${seq++}`} className="border-border/60" />);
    } else if (line.startsWith("### ")) {
      flushItems();
      blocks.push(
        <p key={`h-${seq++}`} className="font-medium text-foreground">
          {line.slice(4)}
        </p>,
      );
    } else if (line.startsWith("- ")) {
      items.push(line.slice(2));
    } else {
      flushItems();
      blocks.push(<p key={`p-${seq++}`}>{renderInline(line, "p")}</p>);
    }
  }
  flushItems();

  return (
    <div className="max-h-52 space-y-2 overflow-auto rounded-md border border-border/60 bg-muted/30 p-3 text-xs text-muted-foreground">
      {blocks}
    </div>
  );
}

export function UpdateDialog({
  open,
  onOpenChange,
  phase,
  currentVersion,
  onCheck,
  onDownload,
  onRelaunch,
}: UpdateDialogProps) {
  const info =
    phase.kind === "available" ||
    phase.kind === "downloading" ||
    phase.kind === "installed"
      ? phase.info
      : null;

  const title =
    phase.kind === "available"
      ? "发现新版本"
      : phase.kind === "downloading"
        ? "正在下载更新"
        : phase.kind === "installed"
          ? "更新已安装"
          : phase.kind === "checking"
            ? "正在检查更新"
            : phase.kind === "error"
              ? "检查更新失败"
              : "已是最新版本";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>
            {phase.kind === "available" &&
              "DongX 有新版本可用，是否立即下载安装？"}
            {phase.kind === "downloading" && "下载完成后会自动安装并重启应用。"}
            {phase.kind === "installed" &&
              "新版本已安装，重启应用后生效。"}
            {phase.kind === "checking" && "正在向 GitHub Releases 查询…"}
            {phase.kind === "error" && "未能获取版本信息，可稍后重试。"}
            {phase.kind === "idle" &&
              `当前版本 v${currentVersion || "…"}，已是最新。`}
            {phase.kind === "uptodate" &&
              `当前版本 v${currentVersion || "…"}，已是最新。`}
          </DialogDescription>
        </DialogHeader>

        {phase.kind === "available" && info && (
          <div className="grid gap-3">
            <VersionRow
              from={info.currentVersion}
              to={info.version}
              date={info.date}
            />
            <ReleaseNotes body={info.body} />
          </div>
        )}

        {phase.kind === "downloading" && (
          <div className="grid gap-2">
            <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
              <div
                className="h-full bg-success transition-[width] duration-300"
                style={{
                  width: `${downloadPercent(
                    phase.progress.downloaded,
                    phase.progress.total,
                  )}%`,
                }}
              />
            </div>
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>
                {phase.progress.event === "Started" && "准备下载…"}
                {phase.progress.event === "Progress" &&
                  (phase.progress.total
                    ? `${formatBytes(phase.progress.downloaded)} / ${formatBytes(phase.progress.total)}`
                    : formatBytes(phase.progress.downloaded))}
                {phase.progress.event === "Finished" && "下载完成，正在安装…"}
              </span>
              {phase.progress.total ? (
                <span className="text-success">
                  {downloadPercent(
                    phase.progress.downloaded,
                    phase.progress.total,
                  )}
                  %
                </span>
              ) : null}
            </div>
          </div>
        )}

        {phase.kind === "installed" && info && (
          <VersionRow from={info.currentVersion} to={info.version} />
        )}

        {phase.kind === "error" && (
          <p className="text-xs break-all text-destructive">{phase.message}</p>
        )}

        <DialogFooter>
          {phase.kind === "available" && (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                稍后
              </Button>
              <Button onClick={onDownload}>
                <Download className="mr-1 h-4 w-4" />
                下载并安装
              </Button>
            </>
          )}

          {phase.kind === "downloading" && (
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              后台继续
            </Button>
          )}

          {phase.kind === "installed" && (
            <Button onClick={onRelaunch}>
              <RotateCw className="mr-1 h-4 w-4" />
              重启应用
            </Button>
          )}

          {(phase.kind === "error" || phase.kind === "uptodate") && (
            <Button variant="outline" onClick={onCheck}>
              <RefreshCw className="mr-1 h-4 w-4" />
              重新检查
            </Button>
          )}

          {phase.kind === "checking" && (
            <Button variant="outline" disabled>
              <RefreshCw className="mr-1 h-4 w-4 animate-spin" />
              检查中…
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
