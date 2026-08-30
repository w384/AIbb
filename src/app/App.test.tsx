import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { App } from "./App";

describe("App", () => {
  it("shows the deterministic local first-run greeting", () => {
    render(<App />);

    expect(screen.getByRole("main", { name: "AIbb" })).toHaveTextContent(
      "你好！我是喜欢出去玩耍的快乐 AIbb。右键点击我，先配置一个大模型 API 吧。",
    );
  });
});
