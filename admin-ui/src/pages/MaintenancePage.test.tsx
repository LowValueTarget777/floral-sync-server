import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { MaintenancePage } from "./MaintenancePage";

describe("MaintenancePage", () => {
  test("renders backup entries and recent log lines", () => {
    const onRestoreBackup = vi.fn();
    render(
      <MaintenancePage
        busy={false}
        backups={[
          {
            fileName: "backup-2026-05-19.sqlite3",
            sizeBytes: 2048,
          },
        ]}
        logs={{
          path: "logs/floral-sync-server.log",
          lines: ["[info] sync server started"],
        }}
        onCreateBackup={vi.fn()}
        onRestoreBackup={onRestoreBackup}
        onRefreshLogs={vi.fn()}
      />,
    );

    expect(screen.getByText("backup-2026-05-19.sqlite3")).toBeDefined();
    expect(screen.getByText("[info] sync server started")).toBeDefined();
    expect(screen.getByRole("button", { name: "创建备份" })).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: "恢复备份 backup-2026-05-19.sqlite3" }));
    expect(onRestoreBackup).toHaveBeenCalledWith("backup-2026-05-19.sqlite3");
  });
});
