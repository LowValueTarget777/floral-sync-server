import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { SettingsPage } from "./SettingsPage";

afterEach(cleanup);

describe("SettingsPage", () => {
  test("shows restart required badges for pending fields", () => {
    render(
      <SettingsPage
        settings={{
          syncListen: ["0.0.0.0:8787"],
          adminListen: ["127.0.0.1:8788"],
          dbPath: "data/floral-sync.sqlite3",
          exportDir: "exports",
          logPath: "logs/floral-sync-server.log",
          logLevel: "info",
          syncToken: "sync-token-123",
          syncTokenConfigured: true,
          adminPasswordConfigured: true,
          adminSessionSecretConfigured: true,
          pendingRestartFields: ["syncListen", "dbPath"],
        }}
        busy={false}
        onSave={vi.fn()}
        onRotateToken={vi.fn()}
        onRestartServer={vi.fn()}
        onChangePassword={vi.fn()}
      />,
    );

    expect(screen.getAllByText("需要重启").length).toBeGreaterThan(0);
    expect(screen.getByText("同步 Token")).toBeDefined();
    expect(screen.getAllByText("已配置 · ••••••••").length).toBeGreaterThan(0);
  });

  test("supports reveal copy and restart actions for the current sync token", async () => {
    const onRestartServer = vi.fn();
    const clipboardWrite = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: clipboardWrite,
      },
    });

    render(
      <SettingsPage
        settings={{
          syncListen: ["0.0.0.0:8787"],
          adminListen: ["127.0.0.1:8788"],
          dbPath: "data/floral-sync.sqlite3",
          exportDir: "exports",
          logPath: "logs/floral-sync-server.log",
          logLevel: "info",
          syncToken: "sync-token-123",
          syncTokenConfigured: true,
          adminPasswordConfigured: true,
          adminSessionSecretConfigured: true,
          pendingRestartFields: [],
        }}
        busy={false}
        onSave={vi.fn()}
        onRotateToken={vi.fn()}
        onRestartServer={onRestartServer}
        onChangePassword={vi.fn()}
      />,
    );

    expect(screen.queryByText("sync-token-123")).toBeNull();

    fireEvent.click(screen.getByText("显示 Token"));
    expect(screen.getByText("sync-token-123")).toBeDefined();

    fireEvent.click(screen.getByText("复制 Token"));
    await waitFor(() => expect(clipboardWrite).toHaveBeenCalledWith("sync-token-123"));

    fireEvent.click(screen.getByText("一键重启服务"));
    expect(onRestartServer).toHaveBeenCalledTimes(1);
  });
});
