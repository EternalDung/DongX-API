import { cn } from "@/lib/utils";

/** Shimmer placeholder block. Use to reserve layout space while loading. */
export function Skeleton({
  className,
  ...props
}: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="skeleton"
      className={cn("animate-pulse rounded-md bg-muted/70", className)}
      {...props}
    />
  );
}
