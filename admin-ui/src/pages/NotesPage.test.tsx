import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { NotesPage } from "./NotesPage";

afterEach(() => {
  cleanup();
});

describe("NotesPage", () => {
  test("updates filters and shows the read-only note detail drawer", () => {
    const handleQueryChange = vi.fn();

    render(
      <NotesPage
        busy={false}
        query={{
          search: "",
          category: "",
          state: "all",
        }}
        categories={["默认", "工作"]}
        notesPage={{
          page: 1,
          pageSize: 20,
          total: 1,
          notes: [
            {
              id: "note-1",
              title: "同步测试",
              category: "默认",
              updatedAt: "2026-05-19T10:00:00Z",
              deletedAt: null,
              deviceId: "device-a",
              revision: 5,
            },
          ],
        }}
        selectedNote={{
          id: "note-1",
          title: "同步测试",
          content: "这是一条只读笔记。",
          category: "默认",
          createdAt: "2026-05-19T09:00:00Z",
          updatedAt: "2026-05-19T10:00:00Z",
          deletedAt: null,
          contentHash: "hash",
          deviceId: "device-a",
          revision: 5,
        }}
        selectedHistory={[
          {
            snapshotId: 1,
            noteId: "note-1",
            revision: 5,
            title: "同步测试",
            content: "这是一条只读笔记。",
            category: "默认",
            createdAt: "2026-05-19T09:00:00Z",
            updatedAt: "2026-05-19T10:00:00Z",
            deletedAt: null,
            contentHash: "hash",
            deviceId: "device-a",
            capturedAt: "2026-05-19T10:00:00Z",
          },
        ]}
        onQueryChange={handleQueryChange}
        onSelectNote={vi.fn()}
        onCloseDetail={vi.fn()}
        onDownloadNote={vi.fn()}
        onDownloadSelected={vi.fn()}
      />,
    );

    fireEvent.change(screen.getByLabelText("搜索"), {
      target: { value: "同步" },
    });
    expect(handleQueryChange).toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("分类"), {
      target: { value: "工作" },
    });
    expect(handleQueryChange).toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("状态"), {
      target: { value: "deleted" },
    });
    expect(handleQueryChange).toHaveBeenCalledWith(
      expect.objectContaining({ state: "deleted" }),
    );

    expect(screen.getByText("这是一条只读笔记。")).toBeDefined();
    expect(screen.getByText("版本历史")).toBeDefined();
  });

  test("selects notes and triggers bulk zip download", () => {
    const handleDownloadSelected = vi.fn();

    render(
      <NotesPage
        busy={false}
        query={{
          search: "",
          category: "",
          state: "all",
        }}
        categories={["默认"]}
        notesPage={{
          page: 1,
          pageSize: 20,
          total: 2,
          notes: [
            {
              id: "note-1",
              title: "同步测试",
              category: "默认",
              updatedAt: "2026-05-19T10:00:00Z",
              deletedAt: null,
              deviceId: "device-a",
              revision: 5,
            },
            {
              id: "note-2",
              title: "第二条",
              category: "默认",
              updatedAt: "2026-05-19T10:05:00Z",
              deletedAt: null,
              deviceId: "device-b",
              revision: 6,
            },
          ],
        }}
        selectedNote={null}
        selectedHistory={[]}
        onQueryChange={vi.fn()}
        onSelectNote={vi.fn()}
        onCloseDetail={vi.fn()}
        onDownloadNote={vi.fn()}
        onDownloadSelected={handleDownloadSelected}
      />,
    );

    const bulkButton = screen.getByRole("button", { name: "下载选中 ZIP" });
    expect((bulkButton as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(screen.getByLabelText("选择笔记 同步测试"));
    expect(screen.getByText("已选 1 条")).toBeDefined();
    expect((bulkButton as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(bulkButton);
    expect(handleDownloadSelected).toHaveBeenCalledWith(["note-1"]);

    fireEvent.click(screen.getByLabelText("全选当前页笔记"));
    fireEvent.click(bulkButton);
    expect(handleDownloadSelected).toHaveBeenLastCalledWith(["note-1", "note-2"]);
  });
});
