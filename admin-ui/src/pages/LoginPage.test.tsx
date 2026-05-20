import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { LoginPage } from "./LoginPage";

describe("LoginPage", () => {
  test("submits the entered password for login", () => {
    const handleSubmit = vi.fn();

    render(
      <LoginPage
        mode="login"
        busy={false}
        error={null}
        onSubmit={handleSubmit}
      />,
    );

    fireEvent.change(screen.getByLabelText("管理员密码"), {
      target: { value: "secret-pass" },
    });
    fireEvent.click(screen.getByRole("button", { name: "登录" }));

    expect(handleSubmit).toHaveBeenCalledWith("secret-pass");
  });
});
