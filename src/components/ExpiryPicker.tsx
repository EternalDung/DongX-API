import { useEffect, useRef, useState } from "react";
import { Calendar as CalendarIcon, ChevronLeft, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";

export interface ExpiryPickerProps {
  /** RFC3339 字符串；null 表示永久有效 */
  value: string | null;
  onChange: (v: string | null) => void;
  className?: string;
}

const QUICK_OPTIONS = [
  { label: "24 小时", days: 1 },
  { label: "30 天", days: 30 },
  { label: "180 天", days: 180 },
  { label: "1 年", days: 365 },
];

// 周一为首列
const WEEKDAYS = ["一", "二", "三", "四", "五", "六", "日"];

function pad(n: number): string {
  return n < 10 ? `0${n}` : `${n}`;
}

function formatDisplay(v: string | null): string {
  if (!v) return "永久有效";
  const d = new Date(v);
  if (Number.isNaN(d.getTime())) return "永久有效";
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

// 选中当天本地 23:59:59 转 RFC3339（auth.rs 接受 RFC3339）
function toRfc3339EndOfDay(y: number, m: number, d: number): string {
  return new Date(y, m, d, 23, 59, 59).toISOString();
}

function buildMonthGrid(viewYear: number, viewMonth: number): (Date | null)[][] {
  const first = new Date(viewYear, viewMonth, 1);
  const startWeekday = (first.getDay() + 6) % 7; // 周一=0
  const daysInMonth = new Date(viewYear, viewMonth + 1, 0).getDate();
  const cells: (Date | null)[] = [];
  for (let i = 0; i < startWeekday; i++) cells.push(null);
  for (let d = 1; d <= daysInMonth; d++) cells.push(new Date(viewYear, viewMonth, d));
  while (cells.length % 7 !== 0) cells.push(null);
  const weeks: (Date | null)[][] = [];
  for (let i = 0; i < cells.length; i += 7) weeks.push(cells.slice(i, i + 7));
  return weeks;
}

export function ExpiryPicker({ value, onChange, className }: ExpiryPickerProps) {
  const [open, setOpen] = useState(false);
  const initial = value ? new Date(value) : new Date();
  const [view, setView] = useState({ y: initial.getFullYear(), m: initial.getMonth() });
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const selectedDate = value ? new Date(value) : null;
  const today = new Date();
  const startOfToday = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
  const weeks = buildMonthGrid(view.y, view.m);

  const pickDay = (d: Date) => {
    onChange(toRfc3339EndOfDay(d.getFullYear(), d.getMonth(), d.getDate()));
    setOpen(false);
  };

  const pickQuick = (days: number) => {
    const d = new Date();
    d.setDate(d.getDate() + days);
    d.setHours(23, 59, 59, 0);
    onChange(d.toISOString());
    setOpen(false);
  };

  const goMonth = (delta: number) => {
    setView((v) => {
      const m = v.m + delta;
      const y = v.y + Math.floor(m / 12);
      const mm = ((m % 12) + 12) % 12;
      return { y, m: mm };
    });
  };

  return (
    <div ref={containerRef} className={cn("relative", className)}>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className={cn(
          "flex w-full items-center gap-2 rounded-md border border-input bg-background px-3 py-2 text-sm",
          "hover:border-primary/50 focus:outline-none focus:ring-1 focus:ring-primary",
        )}
      >
        <CalendarIcon className="h-4 w-4 shrink-0 text-muted-foreground" />
        <span className={cn("flex-1 text-left", !value && "text-muted-foreground")}>
          {formatDisplay(value)}
        </span>
      </button>

      {open && (
        <div className="absolute left-0 top-full z-50 mt-2 w-[280px] rounded-lg border bg-background p-3 shadow-lg">
          {/* 头部：年月 + 翻月 */}
          <div className="mb-2 flex items-center justify-between">
            <button
              type="button"
              onClick={() => goMonth(-1)}
              className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
              aria-label="上个月"
            >
              <ChevronLeft className="h-4 w-4" />
            </button>
            <span className="text-sm font-medium">
              {view.y} 年 {view.m + 1} 月
            </span>
            <button
              type="button"
              onClick={() => goMonth(1)}
              className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
              aria-label="下个月"
            >
              <ChevronRight className="h-4 w-4" />
            </button>
          </div>

          {/* 星期表头 */}
          <div className="mb-1 grid grid-cols-7 text-center text-xs text-muted-foreground">
            {WEEKDAYS.map((w) => (
              <span key={w}>{w}</span>
            ))}
          </div>

          {/* 日期网格 */}
          <div className="grid grid-cols-7 gap-0.5">
            {weeks.flat().map((d, i) => {
              if (!d) return <span key={`empty-${i}`} />;
              const isToday =
                d.getFullYear() === today.getFullYear() &&
                d.getMonth() === today.getMonth() &&
                d.getDate() === today.getDate();
              const isSelected =
                selectedDate != null &&
                d.getFullYear() === selectedDate.getFullYear() &&
                d.getMonth() === selectedDate.getMonth() &&
                d.getDate() === selectedDate.getDate();
              const isPast = d.getTime() < startOfToday;
              return (
                <button
                  key={d.toISOString()}
                  type="button"
                  disabled={isPast}
                  onClick={() => pickDay(d)}
                  className={cn(
                    "h-8 rounded-md text-sm transition-colors",
                    isSelected
                      ? "bg-primary text-primary-foreground"
                      : "hover:bg-accent",
                    isToday && !isSelected && "ring-1 ring-primary",
                    isPast &&
                      "cursor-not-allowed text-muted-foreground/40 hover:bg-transparent",
                  )}
                >
                  {d.getDate()}
                </button>
              );
            })}
          </div>

          {/* 快捷选项 */}
          <div className="mt-3 flex flex-wrap gap-1.5 border-t pt-3">
            {QUICK_OPTIONS.map((q) => (
              <button
                key={q.label}
                type="button"
                onClick={() => pickQuick(q.days)}
                className="rounded-full border border-border px-2.5 py-1 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary"
              >
                {q.label}
              </button>
            ))}
            <button
              type="button"
              onClick={() => {
                onChange(null);
                setOpen(false);
              }}
              className="rounded-full border border-border px-2.5 py-1 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary"
            >
              永久
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
