import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// Auto-cleanup needs vitest globals OR an explicit hook — we use the hook.
afterEach(() => {
  cleanup();
});
