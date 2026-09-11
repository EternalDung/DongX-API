import { useCallback, useState } from "react";
import { Check, Copy } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useToast } from "@/components/ui/toast";
import { cn } from "@/lib/utils";

/**
 * 复制逻辑归一 hook。
 *
 * 三处页面原本各写一份「navigator.clipboard.writeText + toast」逻辑，
 * 现统一收口到此 hook：复制成功/失败给出反馈，并维护 copied 高亮态（1.2s 自动复位）。
 */
export function useCopyToClipboard(options?: { notify?: boolean }) {
  const notify = options?.notify ?? true;
  const { success, error } = useToast();
  const [copied, setCopied] = useState(false);

  const copy = useCallback(
    async (text: string, label?: string): Promise<boolean> => {
      try {
        await navigator.clipboard.writeText(text);
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1200);
        if (notify) success(`已复制${label ?? ""}`);
        return true;
      } catch {
        if (notify) error("复制失败");
        return false;
      }
    },
    [notify, success, error],
  );

  return { copied, copy };
}

type CopyButtonVariant = "pill" | "icon" | "button" | "ghost";

/**
 * 通用复制按钮（归一组件）。
 *
 * - pill：紧凑胶囊（日志页复制 JSON 片段）。
 * - icon：方形 icon 按钮（默认，设置页复制地址）。
 * - button：带「复制」文字的描边按钮（代码块复制）。
 * - ghost：无边框无底色的小图标（嵌在卡片/气泡内部）。
 *   用在已有底色的容器里 —— 描边态会因为按钮底色与容器同色而只剩一圈线。
 *
 * 内部复用 useCopyToClipboard，故 toast 反馈与 copied 高亮行为三处完全一致。
 */
export function CopyButton({
  value,
  label,
  variant = "icon",
  className,
  notify = true,
  title,
}: {
  value: string;
  label?: string;
  variant?: CopyButtonVariant;
  className?: string;
  notify?: boolean;
  title?: string;
}) {
  const { copied, copy } = useCopyToClipboard({ notify });

  if (variant === "pill") {
    return (
      <button
        type="button"
        onClick={() => copy(value, label)}
        title={title ?? "复制"}
        className={cn(
          "flex items-center gap-0.5 rounded-full px-2 py-0.5 text-[10px] font-medium transition-colors",
          copied
            ? "bg-emerald-100 text-emerald-700"
            : "text-muted-foreground hover:bg-muted hover:text-foreground",
          className,
        )}
      >
        {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
        {copied ? "已复制" : "复制"}
      </button>
    );
  }

  if (variant === "ghost") {
    return (
      <button
        type="button"
        onClick={() => copy(value, label)}
        title={title ?? "复制"}
        className={cn(
          "flex h-5 w-5 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground",
          copied && "text-success hover:text-success",
          className,
        )}
      >
        {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
      </button>
    );
  }

  if (variant === "button") {
    return (
      <Button
        variant="outline"
        size="sm"
        onClick={() => copy(value, label)}
        className={cn("gap-1.5", className)}
        title={title ?? "复制"}
      >
        <Copy className="h-3 w-3" />
        复制
      </Button>
    );
  }

  return (
    <Button
      variant="outline"
      size="icon"
      onClick={() => copy(value, label)}
      className={cn("h-9 w-9 shrink-0", className)}
      title={title ?? "复制"}
    >
      {copied ? (
        <Check className="h-3.5 w-3.5 text-success" />
      ) : (
        <Copy className="h-3.5 w-3.5" />
      )}
    </Button>
  );
}
