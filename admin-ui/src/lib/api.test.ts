import { describe, expect, test } from "vitest";
import { createAdminApiClient } from "./api";

describe("createAdminApiClient", () => {
  test("parses filenames for bulk zip downloads", async () => {
    const client = createAdminApiClient(
      "",
      (async () =>
        new Response(new Blob(["zip-data"], { type: "application/zip" }), {
          status: 200,
          headers: {
            "content-type": "application/zip",
            "content-disposition": "attachment; filename=\"floral-sync-notes.zip\"",
          },
        })) as typeof fetch,
    );

    const download = await client.downloadNotesArchive(["note-1", "note-2"]);
    expect(download.fileName).toBe("floral-sync-notes.zip");
    expect(download.blob).toHaveProperty("type", "application/zip");
    expect(download.blob).toHaveProperty("size");
  });
});