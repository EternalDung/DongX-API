// 全局自动更新状态。
//
// 为什么需要它：侧边栏版本卡与「关于」页都要展示更新状态。若各自 check() 一次，
// 会出现「侧边栏说有新版本、关于页却写已是最新」的矛盾；弹窗也必须全局单例，
// 否则两个入口会各弹一个。
//
// 职责：启动静默检查一次 → 持有 phase / progress → 渲染唯一一个 UpdateDialog。
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  checkForUpdate,
  downloadAndInstall,
  getCurrentVersion,
  relaunchApp,
  type DownloadProgress,
  type UpdateInfo,
} from "@/lib/updater";
import { UpdateDialog } from "@/components/update/UpdateDialog";
import { useToast } from "@/components/ui/toast";

export type UpdatePhase =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "uptodate" }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "downloading"; info: UpdateInfo; progress: DownloadProgress }
  | { kind: "installed"; info: UpdateInfo }
  | { kind: "error"; message: string };

interface UpdateContextValue {
  phase: UpdatePhase;
  /** 打包时注入的当前版本（tauri.conf.json 的 version），运行期不变 */
  currentVersion: string;
  /** 有可用更新时的信息；其余状态为 null —— 侧边栏据此决定是否亮图标 */
  info: UpdateInfo | null;
  /** 下载进度，仅 downloading 阶段非空 */
  progress: DownloadProgress | null;
  dialogOpen: boolean;
  openDialog: () => void;
  closeDialog: () => void;
  check: () => Promise<void>;
  startDownload: () => Promise<void>;
}

const UpdateContext = createContext<UpdateContextValue | null>(null);

export function useUpdate(): UpdateContextValue {
  const ctx = useContext(UpdateContext);
  if (!ctx) {
    throw new Error("useUpdate 必须在 <UpdateProvider> 内使用");
  }
  return ctx;
}

export function UpdateProvider({ children }: { children: ReactNode }) {
  const [phase, setPhase] = useState<UpdatePhase>({ kind: "idle" });
  const [currentVersion, setCurrentVersion] = useState("");
  const [dialogOpen, setDialogOpen] = useState(false);
  const toast = useToast();

  // 当前版本读一次即可（打包时注入，运行期不变）
  useEffect(() => {
    void getCurrentVersion().then(setCurrentVersion);
  }, []);

  const check = useCallback(async () => {
    setPhase({ kind: "checking" });
    try {
      const info = await checkForUpdate();
      setPhase(info ? { kind: "available", info } : { kind: "uptodate" });
    } catch (e) {
      // 静默：检查失败不弹 toast 打扰用户，只在用户主动打开弹窗时展示原因
      setPhase({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  // 启动静默检查一次。StrictMode 下 effect 会执行两遍，用 ref 去重避免重复请求。
  const startupChecked = useRef(false);
  useEffect(() => {
    if (startupChecked.current) return;
    startupChecked.current = true;
    void check();
  }, [check]);

  const startDownload = useCallback(async () => {
    if (phase.kind !== "available") return;
    const info = phase.info;
    setPhase({
      kind: "downloading",
      info,
      progress: { event: "Started", downloaded: 0, total: undefined },
    });
    try {
      await downloadAndInstall((p) => {
        setPhase({ kind: "downloading", info, progress: p });
      });
      // mac/linux 上 downloadAndInstall 内部已 relaunch() 结束进程，windows 上
      // NSIS 安装器会自启 —— 能走到这里说明环境特殊，交给用户手动重启。
      setPhase({ kind: "installed", info });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(`下载失败：${msg}`);
      setPhase({ kind: "available", info });
    }
  }, [phase, toast]);

  const value = useMemo<UpdateContextValue>(() => {
    const info =
      phase.kind === "available" ||
      phase.kind === "downloading" ||
      phase.kind === "installed"
        ? phase.info
        : null;
    return {
      phase,
      currentVersion,
      info,
      progress: phase.kind === "downloading" ? phase.progress : null,
      dialogOpen,
      openDialog: () => setDialogOpen(true),
      closeDialog: () => setDialogOpen(false),
      check,
      startDownload,
    };
  }, [phase, currentVersion, dialogOpen, check, startDownload]);

  return (
    <UpdateContext.Provider value={value}>
      {children}
      <UpdateDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        phase={phase}
        currentVersion={currentVersion}
        onCheck={check}
        onDownload={startDownload}
        onRelaunch={relaunchApp}
      />
    </UpdateContext.Provider>
  );
}
