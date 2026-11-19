import { AppWindow, Bot, Camera, GitBranch, KeyRound, Palette, Server, Settings, Shield, UserRound } from "lucide-react";

import { useAgentSettings, useIsolationSettings } from "../lib/queries";
import { type SettingsTab, useUi } from "../lib/store";
import { AgentSettingsDialog } from "./AgentSettingsDialog";
import { ClaudeAccounts } from "./ClaudeAccounts";
import { AppearanceSettings } from "./AppearanceSettings";
import { GeneralSettings } from "./GeneralSettings";
import { GitSettings } from "./GitSettings";
import { IsolationDialog } from "./IsolationDialog";
import { OpenInSettings } from "./OpenInSettings";
import { SecretsEditor } from "./SecretsEditor";
import { SnapshotSettings } from "./SnapshotSettings";
import { SshHostsSettings } from "./SshHostsSettings";
import { type SettingsPage, SettingsWindow } from "./SettingsWindow";
import { Dialog, EmbeddedDialog } from "./ui";

const tabs: SettingsPage<SettingsTab>[] = [
  { id: "general", label: "General", icon: <Settings size={14} /> },
  { id: "appearance", label: "Appearance", icon: <Palette size={14} /> },
  { id: "agents", label: "Agents", icon: <Bot size={14} /> },
  { id: "accounts", label: "Accounts", icon: <UserRound size={14} /> },
  { id: "git", label: "Git", icon: <GitBranch size={14} /> },
  { id: "secrets", label: "Secrets", icon: <KeyRound size={14} /> },
  { id: "isolation", label: "Isolation", icon: <Shield size={14} /> },
  { id: "snapshots", label: "Snapshots", icon: <Camera size={14} /> },
  { id: "sshHosts", label: "SSH hosts", icon: <Server size={14} /> },
  { id: "openIn", label: "Open in", icon: <AppWindow size={14} /> },
];

/** All settings in one window. Changes are saved as they're made. */
export function SettingsView({ tab }: { tab: SettingsTab }) {
  const openDialog = useUi((s) => s.openDialog);
  const closeDialog = useUi((s) => s.closeDialog);
  const { data: agents } = useAgentSettings();
  const { data: isolation } = useIsolationSettings();

  return (
    <SettingsWindow
      label="Settings"
      heading="Settings"
      pages={tabs}
      current={tab}
      onSelect={(next) => openDialog({ kind: "settings", tab: next })}
      onClose={closeDialog}
    >
      <EmbeddedDialog.Provider value>
        {tab === "general" && <GeneralSettings />}
        {tab === "appearance" && <AppearanceSettings />}
        {tab === "git" && <GitSettings />}
        {tab === "agents" && agents && <AgentSettingsDialog settings={agents} onClose={closeDialog} />}
        {tab === "accounts" && agents && (
          <Dialog
            title="Claude Code accounts"
            info="Logins of worki's own for Docker (sbx), Docker, Podman and Apple container sessions. Projects use the default one unless they pick another in their settings, where they also choose how they sign in."
            onClose={closeDialog}
          >
            <ClaudeAccounts accounts={agents.accounts} defaultId={agents.defaultAccount} />
          </Dialog>
        )}
        {tab === "secrets" && (
          <Dialog
            title="Secrets"
            info="Every agent gets these as environment variables, wherever it runs. A project's own secret with the same name wins; projects add theirs in their settings. Stored in your Keychain."
            onClose={closeDialog}
          >
            <SecretsEditor scope="global" />
          </Dialog>
        )}
        {tab === "isolation" && isolation && (
          <IsolationDialog current={isolation.default} onClose={closeDialog} />
        )}
        {tab === "snapshots" && <SnapshotSettings />}
        {tab === "sshHosts" && <SshHostsSettings />}
        {tab === "openIn" && <OpenInSettings />}
      </EmbeddedDialog.Provider>
    </SettingsWindow>
  );
}
