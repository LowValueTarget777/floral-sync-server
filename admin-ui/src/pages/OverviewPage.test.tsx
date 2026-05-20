import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { OverviewPage } from "./OverviewPage";

describe("OverviewPage", () => {
  test("renders the overview stat cards and activity summary", () => {
    render(
      <OverviewPage
        overview={{
          latestRevision: 24,
          noteCount: 12,
          deletedNoteCount: 2,
          categoryCount: 4,
          latestSnapshotAt: "2026-05-19T10:00:00Z",
          syncListen: ["0.0.0.0:8787"],
          adminListen: ["127.0.0.1:8788"],
          dbPath: "data/floral-sync.sqlite3",
          exportDir: "exports",
          logPath: "logs/floral-sync-server.log",
          logLevel: "info",
          recentActivitySummary: "最近 10 分钟内有 2 次同步写入。",
        }}
        busy={false}
        onRefresh={vi.fn()}
      />,
    );

    expect(screen.getByText("12")).toBeDefined();
    expect(screen.getByText("2")).toBeDefined();
    expect(screen.getByText("4")).toBeDefined();
    expect(screen.getByText("最近 10 分钟内有 2 次同步写入。")).toBeDefined();
    expect(screen.getByText("0.0.0.0:8787")).toBeDefined();
  });
});
