import { useState } from "react";
import { Calendar as CalendarIcon, X } from "lucide-react";
import { format } from "date-fns";
import { cn } from "@/lib/utils";
import { Calendar } from "@/components/ui/calendar";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Button } from "@/components/ui/button";

export interface DatePickerProps {
  /** "yyyy-MM-dd" 或空串表示未选 */
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  className?: string;
}

// 通用单日期选择器：与「添加密钥」的 ExpiryPicker 同款 Calendar + Popover 外观，
// 但用于日志筛选——允许选择过去日期、无「永久有效」/快捷选项，并额外提供 × 清除。
export function DatePicker({ value, onChange, placeholder = "选择日期", className }: DatePickerProps) {
  const [open, setOpen] = useState(false);
  const selected = value ? new Date(`${value}T00:00:00`) : undefined;
  const today = new Date();

  const handleSelect = (d: Date | undefined) => {
    if (!d) return;
    onChange(format(d, "yyyy-MM-dd"));
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
          <span className="flex-1 truncate">{value ? value : placeholder}</span>
          {value && (
            <span
              role="button"
              tabIndex={-1}
              title="清除日期"
              onClick={(e) => {
                e.stopPropagation();
                e.preventDefault();
                onChange("");
              }}
              className="ml-1 flex h-4 w-4 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            >
              <X className="h-3.5 w-3.5" />
            </span>
          )}
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-auto p-3" align="start">
        <Calendar
          mode="single"
          selected={selected}
          onSelect={handleSelect}
          defaultMonth={selected ?? today}
          captionLayout="dropdown"
          startMonth={new Date(today.getFullYear() - 5, today.getMonth())}
          endMonth={new Date(today.getFullYear() + 1, today.getMonth())}
        />
      </PopoverContent>
    </Popover>
  );
}
