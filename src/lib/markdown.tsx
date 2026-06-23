// A deliberately tiny markdown renderer for release notes — paragraphs,
// `#`–`###` headings, `-`/`*` bullet lists, and `**bold**`. It renders to React
// elements (never dangerouslySetInnerHTML), so untrusted-looking text can't
// inject markup. Anything fancier than the above renders as plain text.
// Used by the changelog viewer and the self-update "what's new" panel.
import type { ReactNode } from "react";

function renderInline(text: string): ReactNode[] {
  return text.split(/(\*\*[^*]+\*\*)/g).map((part, i) => {
    const bold = /^\*\*([^*]+)\*\*$/.exec(part);
    return bold ? <strong key={i}>{bold[1]}</strong> : <span key={i}>{part}</span>;
  });
}

export function Markdown({ text }: { text: string }) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let i = 0;
  const isBullet = (s: string) => /^[-*]\s+/.test(s);
  const isHeading = (s: string) => /^#{1,6}\s+/.test(s);

  while (i < lines.length) {
    const line = lines[i];
    if (line.trim() === "") {
      i++;
      continue;
    }

    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      blocks.push(
        <div
          key={blocks.length}
          style={{ fontWeight: 600, fontSize: heading[1].length <= 2 ? 14 : 13, marginTop: 10 }}
        >
          {renderInline(heading[2])}
        </div>,
      );
      i++;
      continue;
    }

    if (isBullet(line)) {
      const items: string[] = [];
      while (i < lines.length && isBullet(lines[i])) {
        items.push(lines[i].replace(/^[-*]\s+/, ""));
        i++;
      }
      blocks.push(
        <ul key={blocks.length} style={{ margin: "4px 0", paddingLeft: 18 }}>
          {items.map((item, j) => (
            <li key={j} style={{ marginBottom: 2 }}>
              {renderInline(item)}
            </li>
          ))}
        </ul>,
      );
      continue;
    }

    const para: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() !== "" &&
      !isBullet(lines[i]) &&
      !isHeading(lines[i])
    ) {
      para.push(lines[i]);
      i++;
    }
    blocks.push(
      <p key={blocks.length} style={{ margin: "4px 0" }}>
        {renderInline(para.join(" "))}
      </p>,
    );
  }

  return <>{blocks}</>;
}
