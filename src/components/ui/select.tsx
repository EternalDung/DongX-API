import * as React from "react"

import { cn } from "@/lib/utils"

/**
 * 轻量 Select — 原生 <select> + shadcn 样式
 * 复杂场景（搜索、分组）再升级为 Radix Select
 */
function Select({ className, children, ...props }: React.ComponentProps<"select">) {
  return (
    <select
      data-slot="select"
      className={cn(
        "border-input dark:bg-input/30 dark:hover:bg-input/50 flex h-9 w-full min-w-0 rounded-md border bg-transparent px-3 py-1 text-sm shadow-xs transition-[color,box-shadow] outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50",
        "[&>option]:bg-background [&>option]:text-foreground",
        className
      )}
      {...props}
    >
      {children}
    </select>
  )
}

export { Select }
