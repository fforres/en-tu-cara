// Full release history (Settings → About), from the in-code changelog/. Each
// version is a clickable header that expands to its notes; the running version
// is marked "current". See src/lib/changelog.ts.
import { useState } from "react";
import { releases } from "../../lib/changelog";
import { Markdown } from "../../lib/markdown";
import { css } from "./styles";

export function ChangelogViewer() {
  const [open, setOpen] = useState<Set<string>>(() => new Set());
  const toggle = (version: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(version)) {
        next.delete(version);
      } else {
        next.add(version);
      }
      return next;
    });

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        width: 360,
        maxWidth: "100%",
        border: css.hairline,
        borderRadius: 8,
        overflow: "hidden",
      }}
    >
      {releases.map((r, i) => {
        const expanded = open.has(r.version);
        const current = r.version === __APP_VERSION__;
        return (
          <div key={r.version} style={{ borderTop: i === 0 ? undefined : css.hairline }}>
            <button
              onClick={() => toggle(r.version)}
              aria-expanded={expanded}
              style={{
                font: "inherit",
                width: "100%",
                display: "flex",
                alignItems: "baseline",
                gap: 8,
                padding: "8px 12px",
                background: "transparent",
                border: "none",
                cursor: "pointer",
                textAlign: "left",
              }}
            >
              <span style={{ color: css.secondary, fontSize: 11 }}>{expanded ? "▾" : "▸"}</span>
              <span style={{ fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>
                v{r.version}
              </span>
              {current && (
                <span
                  style={{
                    fontSize: 10,
                    fontWeight: 600,
                    color: css.secondary,
                    border: css.hairline,
                    borderRadius: 4,
                    padding: "0 5px",
                  }}
                >
                  current
                </span>
              )}
              <span style={{ flex: 1, fontSize: 12, color: css.secondary, minWidth: 0 }}>
                {r.title}
              </span>
              <span
                style={{ fontSize: 11, color: css.secondary, fontVariantNumeric: "tabular-nums" }}
              >
                {r.date}
              </span>
            </button>
            {expanded && (
              <div style={{ padding: "0 12px 10px 30px", fontSize: 12, lineHeight: 1.45 }}>
                <Markdown text={r.body} />
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
