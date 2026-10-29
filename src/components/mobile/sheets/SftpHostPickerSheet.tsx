import { useTranslation } from "react-i18next";
import { MyDevicesGroup } from "@/components/shared/MyDevicesGroup";
import type { SftpPaneTarget } from "../panels/sftpSelectionCore";
import HostPickerSheet from "./HostPickerSheet";

/** SFTP's host picker: SSH-capable hosts and the account's other devices. */
export default function SftpHostPickerSheet({
  excludeId, onPick, onClose,
}: { excludeId?: string; onPick: (target: SftpPaneTarget) => void; onClose: () => void }) {
  const { t } = useTranslation();
  return (
    <HostPickerSheet title={t("mobile.sftp.chooseHost")} attr="sftp-host" emptyLabel={t("mobile.sheets.sftpHostPicker.noSshHosts")}
      sshOnly excludeId={excludeId} onClose={onClose}
      devices={(query) => <MyDevicesGroup density="touch" query={query} onPick={(device) => onPick({ kind: "device", device })} />}
      onPick={(id) => onPick({ kind: "connection", id })} />
  );
}
