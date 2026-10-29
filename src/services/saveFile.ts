import { invoke } from "@/lib/invoke";
import { saveSystemDialog as save } from "@/services/systemDialog";
import i18n from "@/i18n";
import { appToast } from "@/services/appToast";
import { getPlatformSync } from "@/utils/platform";

const MIME: Record<string, string> = { json: "application/json", csv: "text/csv" };

export function notify(severity: "success" | "error", message: string) {
  appToast(severity, message);
}

function blobDownload(filename: string, content: string, ext: string) {
  const url = URL.createObjectURL(new Blob([content], { type: MIME[ext] ?? "text/plain" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

/** Asks where to save `content`, writes it, and toasts the outcome. Resolves false when cancelled or failed. */
export async function saveTextFile(filename: string, content: string): Promise<boolean> {
  const ext = filename.slice(filename.lastIndexOf(".") + 1).toLowerCase();
  // The Android WebView has no writable native save path, so the blob `<a download>` lands in
  // the system Downloads folder; iOS has a real save sheet, so it uses the dialog + fs_write_file.
  if (getPlatformSync() === "android") {
    blobDownload(filename, content, ext);
    return true;
  }
  try {
    const path = await save({ defaultPath: filename, filters: [{ name: ext.toUpperCase(), extensions: [ext] }] });
    if (!path) return false;
    await invoke("fs_write_file", { path, content });
    notify("success", i18n.t("common.toast.fileSaved", { path }));
    return true;
  } catch (e) {
    notify("error", i18n.t("common.toast.fileSaveFailed", { error: e instanceof Error ? e.message : String(e) }));
    return false;
  }
}
