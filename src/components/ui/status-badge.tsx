import { Badge } from "./badge";
import { cn } from "@/lib/utils";

export type StatusTone =
  | "success"
  | "warning"
  | "destructive"
  | "info"
  | "secondary";

const DOT: Record<StatusTone, string> = {
  success: "bg-success",
  warning: "bg-warning",
  destructive: "bg-destructive",
  info: "bg-primary",
  secondary: "bg-muted-foreground",
};

const VARIANT: Record<StatusTone, "success" | "warning" | "destructive" | "secondary"> = {
  success: "success",
  warning: "warning",
  destructive: "destructive",
  info: "secondary",
  secondary: "secondary",
};

interface StatusBadgeProps {
  tone?: StatusTone;
  children: React.ReactNode;
  dot?: boolean;
  className?: string;
}

/** Badge with a leading status dot. Keeps status colors consistent app-wide. */
export function StatusBadge({
  tone = "secondary",
  children,
  dot = true,
  className,
}: StatusBadgeProps) {
  return (
    <Badge variant={VARIANT[tone]} className={cn("gap-1.5", className)}>
      {dot && <span className={cn("h-1.5 w-1.5 rounded-full", DOT[tone])} />}
      {children}
    </Badge>
  );
}
