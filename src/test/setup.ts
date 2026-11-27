/// <reference types="vitest" />
import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

// vitest 未开启 globals，Testing Library 的自动清理不会注册，手动挂上。
afterEach(() => {
  cleanup();
});
