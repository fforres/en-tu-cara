import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { TrayPopover } from "./TrayPopover";

describe("TrayPopover (Phase 0 stub)", () => {
  it("renders", () => {
    render(<TrayPopover />);
    expect(screen.getByText("En Tu Cara")).toBeInTheDocument();
  });
});
