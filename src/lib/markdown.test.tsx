import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { Markdown } from "./markdown";

describe("Markdown", () => {
  it("renders paragraphs, headings, bullets, and bold", () => {
    const { container } = render(
      <Markdown text={"## What's new\n\nA **big** change:\n\n- first\n- second\n"} />,
    );
    expect(screen.getByText("What's new")).toBeInTheDocument();
    expect(screen.getByText("big").tagName).toBe("STRONG");
    expect(container.querySelectorAll("li")).toHaveLength(2);
    expect(container.querySelector("p")).toBeTruthy();
  });

  it("joins wrapped paragraph lines with spaces", () => {
    render(<Markdown text={"line one\nline two"} />);
    expect(screen.getByText("line one line two")).toBeInTheDocument();
  });

  it("does not render raw HTML as markup", () => {
    const { container } = render(<Markdown text={"<b>not bold</b>"} />);
    expect(container.querySelector("b")).toBeNull();
    expect(screen.getByText("<b>not bold</b>")).toBeInTheDocument();
  });
});
