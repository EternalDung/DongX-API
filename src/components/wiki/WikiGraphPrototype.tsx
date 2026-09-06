import { useEffect, useMemo, useRef, useState } from "react";
import { Graph } from "@antv/g6";
import { Network, Loader2, Maximize } from "lucide-react";
import { wikiApi } from "@/lib/api";
import type { WikiPage, WikiPageKind } from "@/types";

// 与各页面 Tab 的 PAGE_KIND_STYLE 语义色一致（此处需具体色值，g6 画布不吃 tailwind class）
const KIND_COLOR: Record<WikiPageKind, string> = {
  索引: "#8b5cf6",
  概念: "#0ea5e9",
  实体: "#10b981",
  日志: "#f59e0b",
  摘要: "#64748b",
};
const KIND_ORDER: WikiPageKind[] = ["索引", "概念", "实体", "日志", "摘要"];

function buildGraph(pages: WikiPage[]) {
  const titleSet = new Set(pages.map((p) => p.title));
  const nodes = pages.map((p) => ({
    id: p.title,
    data: { label: p.title, kind: p.kind },
  }));
  const seen = new Set<string>();
  const edges: { source: string; target: string }[] = [];
  for (const p of pages) {
    for (const t of p.links) {
      if (t === p.title || !titleSet.has(t)) continue; // 仅连真实存在的页，避免悬空边
      const key = [p.title, t].sort().join("::");
      if (seen.has(key)) continue;
      seen.add(key);
      edges.push({ source: p.title, target: t });
    }
  }
  return { nodes, edges };
}

/**
 * Wiki 知识图谱原型（antv/g6 v5）。
 * 数据来自 mock 的页面 wikilink 关系；后端 019 落地后把 wikiApi.pages 换成真实接口即可，组件无需改动。
 */
export default function WikiGraphPrototype({ projectId }: { projectId: string }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const graphRef = useRef<Graph | null>(null);
  const [pages, setPages] = useState<WikiPage[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    wikiApi
      .pages(projectId)
      .then((ps) => {
        if (alive) {
          setPages(ps);
          setLoading(false);
        }
      })
      .catch(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, [projectId]);

  const graphData = useMemo(() => buildGraph(pages), [pages]);

  useEffect(() => {
    if (!containerRef.current || graphData.nodes.length === 0) return;
    const graph = new Graph({
      container: containerRef.current,
      autoResize: true,
      autoFit: "view",
      data: graphData,
      node: {
        style: {
          size: 26,
          fill: (d: any) =>
            KIND_COLOR[(d.data?.kind as WikiPageKind) ?? "摘要"] ?? KIND_COLOR["摘要"],
          stroke: "#0f172a",
          lineWidth: 1.5,
          labelText: (d: any) => String(d.data?.label ?? ""),
          labelPlacement: "bottom",
          labelFill: "#cbd5e1",
          labelFontSize: 11,
          labelBackground: true,
          labelBackgroundFill: "rgba(15,23,42,0.65)",
        },
      },
      edge: {
        type: "line",
        style: {
          stroke: "rgba(148,163,184,0.4)",
          lineWidth: 1,
          endArrow: true,
        },
      },
      layout: {
        type: "d3-force",
        collide: { radius: 38 },
        link: { distance: 110, strength: 0.5 },
        manyBody: { strength: -280 },
      },
      behaviors: ["zoom-canvas", "drag-canvas", "drag-element", "hover-activate"],
    });
    graph.render();
    graphRef.current = graph;
    return () => {
      graph.destroy();
      graphRef.current = null;
    };
  }, [graphData]);

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Network className="h-4 w-4" />
          <span>知识图谱 · 基于页面间 [[wikilink]] 引用关系（antv/g6 原型）</span>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          {KIND_ORDER.map((k) => (
            <span key={k} className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <span
                className="h-2.5 w-2.5 rounded-full"
                style={{ backgroundColor: KIND_COLOR[k] }}
              />
              {k}
            </span>
          ))}
        </div>
      </div>

      <div className="relative h-[520px] w-full overflow-hidden rounded-lg border border-border bg-[#0b1120]">
        {loading && (
          <div className="absolute inset-0 flex items-center justify-center text-muted-foreground">
            <Loader2 className="mr-2 h-4 w-4 animate-spin" /> 加载图谱数据…
          </div>
        )}
        {!loading && graphData.nodes.length === 0 && (
          <div className="absolute inset-0 flex items-center justify-center text-sm text-muted-foreground">
            暂无页面数据
          </div>
        )}
        <div ref={containerRef} className="h-full w-full" />
        <button
          type="button"
          onClick={() => graphRef.current?.fitView()}
          title="重置缩放 / 适配视图"
          className="absolute bottom-3 right-3 z-10 flex h-9 w-9 items-center justify-center rounded-md border border-border bg-background/80 text-muted-foreground shadow-sm backdrop-blur transition-colors hover:bg-accent hover:text-foreground"
        >
          <Maximize className="h-4 w-4" />
        </button>
      </div>

      <p className="text-xs text-muted-foreground">
        滚轮缩放 · 拖拽空白平移画布 · 拖拽节点重排 · 悬停高亮关联
      </p>
    </div>
  );
}
