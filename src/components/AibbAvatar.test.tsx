import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AibbAvatar } from "./AibbAvatar";

describe("AibbAvatar", () => {
  it("renders a saved profile image with the nickname as its accessible text", () => {
    render(<AibbAvatar avatarDataUrl="data:image/webp;base64,AQID" name="小团子" />);

    expect(screen.getByRole("img", { name: "小团子" })).toHaveAttribute(
      "src",
      "data:image/webp;base64,AQID",
    );
  });

  it("keeps the built-in robot when there is no saved image", () => {
    const { container } = render(<AibbAvatar name="小团子" />);

    expect(container.querySelector("svg.aibb-avatar")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "小团子" })).not.toBeInTheDocument();
  });
});
