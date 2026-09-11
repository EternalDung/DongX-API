// 「关于」标签页：应用信息 + GitHub 仓库入口。
//
// 职责边界（刻意收窄）：这里只回答「这是什么 / 去哪儿找」，不承载更新动作本身 ——
// 更新由全局 UpdateDialog 负责（侧边栏版本卡或本页「检查更新」唤起），
// 因此原先内嵌的更新卡片已移除，避免与全局状态重复、出现两套说法。
//
// 保留两个外链按钮的原因：自动更新失败（网络不可达 / 签名不匹配 / 自建 fork）时，
// 用户必须有一条去 Releases 手动下载的出路，否则就是死路。
import { ExternalLink, GitBranch, Info, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useUpdate } from "@/lib/update-store";

/** 项目仓库地址（latest.json 与安装包都发布在这里） */
export const REPO_URL = "https://github.com/EternalDung/DongX-API";
/** 手动下载页（Releases 列表） */
export const RELEASES_URL = `${REPO_URL}/releases`;

export function AboutSection() {
  // 版本与更新动作都来自全局 store，本页不再自行 check、不持有独立状态
  const { currentVersion, check, openDialog } = useUpdate();

  const openExternal = async (url: string) => {
    try {
      await openUrl(url);
    } catch {
      // 非 Tauri 环境（如 vitest）或系统未注册处理器时静默失败
    }
  };

  // 先打开弹窗（展示「正在检查」），检查结果由 store 回填到同一个弹窗
  const handleCheck = () => {
    openDialog();
    void check();
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Info className="h-4 w-4" /> 关于 DongX
        </CardTitle>
        <CardDescription>本地 LLM API 网关 · 桌面版</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        <div className="flex flex-wrap items-center gap-3">
          <span className="text-2xl font-semibold tracking-tight">DongX</span>
          <Badge variant="secondary">v{currentVersion || "—"}</Badge>
          <Button size="sm" variant="outline" onClick={handleCheck}>
            <RefreshCw className="mr-1 h-3.5 w-3.5" />
            检查更新
          </Button>
        </div>

        <div className="flex flex-wrap items-center gap-2 text-sm">
          <GitBranch className="h-4 w-4 text-muted-foreground" />
          <span className="font-mono text-xs text-muted-foreground">
            EternalDung/DongX-API
          </span>
          <Button
            size="sm"
            variant="outline"
            onClick={() => openExternal(REPO_URL)}
          >
            <ExternalLink className="mr-1 h-3.5 w-3.5" />
            项目主页
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => openExternal(RELEASES_URL)}
          >
            <ExternalLink className="mr-1 h-3.5 w-3.5" />
            手动下载
          </Button>
        </div>

        <p className="text-xs text-muted-foreground">
          自动更新失败时，可点「手动下载」前往 GitHub Releases 下载最新安装包覆盖安装。
        </p>
      </CardContent>
    </Card>
  );
}
