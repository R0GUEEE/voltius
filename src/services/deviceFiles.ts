import { listen } from "@tauri-apps/api/event";
import { invoke } from "@/lib/invoke";
import { logFailure } from "@/lib/logger";
import { getAccountId } from "@/services/account";
import { getPlatform } from "@/utils/platform";
import {
  useDeviceFilesStore,
  type ActiveSession,
  type ApprovalPrompt,
  type FileAccessConfig,
  type FileAnswer,
} from "@/stores/deviceFilesStore";

const store = () => useDeviceFilesStore.getState();
const fail = (context: string) => logFailure(`device-files ${context}`);

let offs: Array<() => void> = [];
let starting: Promise<void> | null = null;

async function refreshPending(): Promise<void> {
  store().setPending(await invoke<ApprovalPrompt[]>("device_files_pending"));
}

const refresh = () => refreshPending().catch(fail("pending"));

async function begin(): Promise<void> {
  if ((await getPlatform()) === "android") return;
  const accountId = await getAccountId();
  if (!accountId) return;
  await invoke("device_files_start", { accountId });
  const next: Array<() => void> = [
    await listen("device-files://request", () => void refresh()),
    await listen<ActiveSession[]>("device-files://sessions", (e) => store().setSessions(e.payload)),
    await listen<FileAccessConfig>("device-files://config", (e) => store().setConfig(e.payload)),
  ];
  window.addEventListener("focus", refresh);
  next.push(() => window.removeEventListener("focus", refresh));
  offs = next;
  store().setConfig(await invoke<FileAccessConfig>("device_files_get_config"));
  await refreshPending();
}

export function startDeviceFiles(): Promise<void> {
  if (offs.length > 0) return Promise.resolve();
  starting ??= begin().finally(() => {
    starting = null;
  });
  return starting;
}

export async function stopDeviceFiles(): Promise<void> {
  offs.forEach((off) => off());
  offs = [];
  store().reset();
  if ((await getPlatform()) === "android") return;
  await invoke("device_files_stop").catch(fail("stop"));
}

export function connectDevice(deviceId: string, connectId: string, relink?: string): Promise<{ sftpId: string; rttMs: number }> {
  return invoke("device_files_connect", { deviceId, connectId, relink });
}

export async function answerFileRequest(requestId: string, answer: FileAnswer): Promise<void> {
  try {
    await invoke("device_files_answer", { requestId, answer });
  } finally {
    await refresh();
  }
}

export async function saveFileAccess(config: FileAccessConfig): Promise<void> {
  await invoke("device_files_set_config", { config });
  store().setConfig(config);
}

export function disconnectDevice(deviceKey: string): Promise<void> {
  return invoke("device_files_disconnect", { deviceKey });
}
