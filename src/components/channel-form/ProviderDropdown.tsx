import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { ChannelPreset } from "@/types";
import { CHANNEL_CATEGORIES, CHANNEL_PROVIDER_ICONS } from "@/lib/provider-icons";

const REGION_ORDER = ["custom", "international", "domestic", "local"] as const;

/**
 * Provider picker — grouped dropdown driven by the backend preset registry.
 * Renders real brand SVG icons (CHANNEL_PROVIDER_ICONS) and four region groups
 * (custom / international / domestic / local) following the protocol selected
 * in the parent form.
 */
export function ProviderDropdown({
  presets,
  current,
  onSelect,
}: {
  presets: ChannelPreset[];
  current: string;
  onSelect: (p: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [focusIdx, setFocusIdx] = useState(-1);
  const rootRef = useRef<HTMLDivElement>(null);

  const onSelectRef = useRef(onSelect);
  useEffect(() => {
    onSelectRef.current = onSelect;
  }, [onSelect]);

  const groups = useMemo(
    () =>
      REGION_ORDER.map((region) => ({
        region,
        presets: presets.filter((p) => p.region === region),
      })).filter((g) => g.presets.length > 0),
    [presets]
  );
  const flat = useMemo(() => groups.flatMap((g) => g.presets), [groups]);

  const flatIndexById = useMemo(() => {
    const m = new Map<string, number>();
    flat.forEach((p, i) => m.set(p.id, i));
    return m;
  }, [flat]);

  const listboxId = useId();
  const currentPreset = presets.find((p) => p.provider === current) ?? presets[0];

  useEffect(() => {
    if (!open) return;
    const onDocClick = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("click", onDocClick);
    return () => document.removeEventListener("click", onDocClick);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (flat.length === 0) return;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setFocusIdx((i) => (i + 1) % flat.length);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setFocusIdx((i) => (i <= 0 ? flat.length - 1 : i - 1));
      } else if (e.key === "Enter") {
        const f = flat[focusIdx];
        if (f) {
          onSelectRef.current(f.provider);
          setOpen(false);
        }
      } else if (e.key === "Escape") {
        setOpen(false);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, focusIdx, groups]);

  if (!currentPreset) return null;

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listboxId}
        aria-activedescendant={open ? flat[focusIdx]?.id : undefined}
        onClick={() => {
          setOpen((o) => !o);
          setFocusIdx(-1);
        }}
        className={`flex w-full items-center gap-2.5 rounded-lg border bg-background px-3 py-2.5 text-left transition-colors ${
          open
            ? "border-primary ring-1 ring-primary/30"
            : "border-border hover:border-primary/50"
        }`}
      >
        <span className="flex h-6 w-6 shrink-0 items-center justify-center">
          <span
            className="flex h-5 w-5 items-center justify-center"
            dangerouslySetInnerHTML={{
              __html: CHANNEL_PROVIDER_ICONS[currentPreset.icon_key] ?? "",
            }}
          />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-semibold">
            {currentPreset.display_name}
          </span>
          <span className="block truncate text-xs text-muted-foreground">
            {currentPreset.description}
          </span>
        </span>
        <span
          className={`shrink-0 text-muted-foreground transition-transform ${
            open ? "rotate-180" : ""
          }`}
        >
          ▾
        </span>
      </button>

      {open && (
        <div
          id={listboxId}
          role="listbox"
          className="absolute left-0 right-0 top-[calc(100%+6px)] z-50 max-h-80 overflow-y-auto rounded-lg border border-border bg-background p-1.5 shadow-md"
        >
          {groups.map((g) => (
            <div
              key={g.region}
              className={g.region !== "custom" ? "mt-1 border-t border-border pt-1" : ""}
            >
              <div className="px-2.5 pb-1 pt-2 text-[11px] font-bold tracking-wider text-muted-foreground">
                {CHANNEL_CATEGORIES[g.region]?.icon} {CHANNEL_CATEGORIES[g.region]?.label}
              </div>
              <div
                className={`grid gap-1 ${
                  g.region === "custom" ? "grid-cols-1" : "grid-cols-2"
                }`}
              >
                {g.presets.map((p) => {
                  const flatIdx = flatIndexById.get(p.id) ?? 0;
                  const isCurrent = p.provider === current;
                  const isFocused = flatIdx === focusIdx;
                  return (
                    <button
                      key={p.id}
                      id={p.id}
                      type="button"
                      role="option"
                      aria-selected={isCurrent}
                      title={p.description}
                      onClick={() => {
                        onSelect(p.provider);
                        setOpen(false);
                      }}
                      onMouseEnter={() => setFocusIdx(flatIdx)}
                      className={`flex items-center gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors ${
                        isFocused
                          ? "bg-muted ring-1 ring-primary/30"
                          : isCurrent
                            ? "bg-primary/10"
                            : "hover:bg-muted"
                      }`}
                    >
                      <span className="flex h-[18px] w-[18px] shrink-0 items-center justify-center">
                        <span
                          className="flex h-[18px] w-[18px] items-center justify-center"
                          dangerouslySetInnerHTML={{
                            __html: CHANNEL_PROVIDER_ICONS[p.icon_key] ?? "",
                          }}
                        />
                      </span>
                      <span className="min-w-0 flex-1">
                        <span
                          className={`block truncate text-[13.5px] font-semibold ${
                            isCurrent ? "text-primary" : ""
                          }`}
                        >
                          {p.display_name}
                        </span>
                      </span>
                      <span className="shrink-0 font-bold text-primary">
                        {isCurrent ? "✓" : ""}
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
