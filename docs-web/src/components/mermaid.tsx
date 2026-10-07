import { renderMermaidSVG } from "beautiful-mermaid";
import { CodeBlock, Pre } from "fumadocs-ui/components/codeblock";

export async function Mermaid({ chart }: { chart: string }) {
  try {
    const svg = renderMermaidSVG(chart, {
      bg: "var(--color-fd-background)",
      fg: "var(--color-fd-foreground)",
      interactive: true,
      transparent: true,
    });

    return (
      <div
        className="mermaid-diagram"
        // biome-ignore lint/security/noDangerouslySetInnerHtml: SVG is rendered server-side from repo-authored MDX, not user input
        dangerouslySetInnerHTML={{ __html: svg }}
      />
    );
  } catch {
    return (
      <CodeBlock title="Mermaid">
        <Pre>{chart}</Pre>
      </CodeBlock>
    );
  }
}
