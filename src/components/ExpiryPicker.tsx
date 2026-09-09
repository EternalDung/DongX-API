import { useState } from "react";
import { Calendar as CalendarIcon } from "lucide-react";
import { format } from "date-fns";
import { cn } from "@/lib/utils";
import { Calendar } from "@/components/ui/calendar";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Button } from "@/components/ui/button";

export interface ExpiryPickerProps {
  /** RFC3339 字符串；null 表示永久有效 */
  value: string | null;
  onChange: (v: string | null) => void;
  className?: string;
}

const QUICK_OPTIONS = [
  { label: "30 天", days: 30 },
  { label: "60 天", days: 60 },
  { label: "180 天", days: 180 },
  { label: "1 年", days: 365 },
];

// 选中当天本地 23:59:59 转 RFC3339（auth.rs 接受 RFC3339，过期判定按天）
function toRfc3339EndOfDay(d: Date): string {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate(), 23, 59, 59).toISOString();
}

function formatDisplay(v: string | null): string {
  if (!v) return "永久有效";
  const d = new Date(v);
  if (Number.isNaN(d.getTime())) return "永久有效";
  return format(d, "yyyy-MM-dd");
}

export function ExpiryPicker({ value, onChange, className }: ExpiryPickerProps) {
  const selected = value ? new Date(value) : undefined;
  const [open, setOpen] = useState(false);

  const today = new Date();
  const startOfToday = new Date(today.getFullYear(), today.getMonth(), today.getDate());

  const handleDaySelect = (d: Date | undefined) => {
    if (!d) return;
    onChange(toRfc3339EndOfDay(d));
    setOpen(false);
  };

  const handleQuick = (days: number) => {
    const d = new Date();
    d.setDate(d.getDate() + days);
    d.setHours(23, 59, 59, 0);
    onChange(d.toISOString());
    setOpen(false);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="outline"
          className={cn(
            "w-full justify-start text-left font-normal",
            !value && "text-muted-foreground",
            className,
          )}
        >
          <CalendarIcon className="h-4 w-4 shrink-0" />
          <span className="flex-1">{formatDisplay(value)}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-auto p-3" align="start">
        <Calendar
          mode="single"
          selected={selected}
          onSelect={handleDaySelect}
          defaultMonth={selected ?? today}
          disabled={{ before: startOfToday }}
          captionLayout="dropdown"
          startMonth={startOfToday}
          endMonth={new Date(today.getFullYear() + 5, today.getMonth())}
        />
        <div className="mt-3 flex flex-wrap gap-1.5 border-t pt-3">
          {QUICK_OPTIONS.map((q) => (
            <button
              key={q.label}
              type="button"
              onClick={() => handleQuick(q.days)}
              className="rounded-full border border-border px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:border-primary/50 hover:text-primary"
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
            className="rounded-full border border-border px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:border-primary/50 hover:text-primary"
          >
            永久
          </button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
