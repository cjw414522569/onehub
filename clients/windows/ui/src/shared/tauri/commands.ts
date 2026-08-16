import { invoke } from "@tauri-apps/api/core";
import type {
  DbCompareResult,
  DbConnectionInput,
  DbConnectionProfile,
  DbConnectResult,
  DbErMetadata,
  DbEngineInfo,
  DbObjectInfo,
  DbProxyRoute,
  MongoCollectionsResult,
  MongoDeleteResult,
  MongoDocumentsResult,
  MongoInsertResult,
  MongoUpdateResult,
  RedisConsoleResult,
  RedisDelResult,
  RedisKeyList,
  RedisKeyValue,
  RedisSetResult,
  RedisTtlResult,
  RedisTypeResult,
  DbQueryRequest,
  DbQueryResult,
  DbTestResult,
} from "../../features/database/dbTypes";
import type {
  NoteAssetResult,
  NoteExportResult,
  NoteDeleteResult,
  NoteReadResult,
  NotesDirResult,
  NotesListResult,
  NoteSaveResult,
} from "../../features/notes/notesTypes";
import type {
  AgentInfo,
  AgentProjectFilesResult,
  AgentStartResult,
  AgentStopResult,
} from "../../features/agent/agentTypes";
import type {
  GitBranchesResult,
  GitDiffResult,
  GitStatusResult,
  GitSwitchResult,
} from "../../features/git/gitTypes";
import type {
  ConnectionRuntimeCredentialRequest,
  ConnectionStepResult,
  CredentialProfile,
  CredentialProfileInput,
  ConnectionProfile,
  ConnectionProfileInput,
  HostKeyInfo,
  RevealedConnectionSecret,
  RevealedCredentialSecret,
  RdpLaunchPreview,
  RdpLaunchResult,
  RdpEmbeddedBounds,
  RdpRunnerConfig,
  RdpRunnerProbeResult,
  RdpSessionCloseResult,
  RdpSessionRevealResult,
  RdpSessionResizeResult,
  VncLaunchPreview,
  VncLaunchResult,
  VncRunnerConfig,
  VncRunnerProbeResult,
  VncSessionCloseResult,
} from "../../features/connections/connectionTypes";
import type {
  ConnectionTransferConflictStrategy,
  ConnectionTransferExportResult,
  ConnectionTransferImportResult,
  ConnectionTransferPreviewResult,
} from "../../features/connections/connectionTransferTypes";
import type {
  AiChatSession,
  AiChatSessionSummary,
  AiChatStreamStartRequest,
  AiChatStreamStartResponse,
  AiCommandAssessment,
  AiProviderConfig,
  AiProviderConfigInput,
  AiProviderModelOption,
  AiProviderConfigTestResult,
  RevealedAiProviderApiKey,
} from "../../features/ai/aiTypes";
import type {
  CommandHistoryEntry,
  CommandHistoryRecordRequest,
  CommandHistoryScope,
  CommandSnippet,
  CommandSnippetInput,
} from "../../features/commands/commandLibraryTypes";
import type {
  DockerActionResult,
  DockerContainerAction,
  DockerContainerDetail,
  DockerContainerSummary,
  DockerEngineAction,
  DockerEngineConfigResult,
  DockerEngineStatus,
  DockerImageRunRequest,
  DockerImageSummary,
  DockerLogsResult,
  DockerNetworkSummary,
  DockerRestartPolicyKind,
  NetworkDiagnosticRequest,
  NetworkDiagnosticResult,
} from "../../features/tools/dockerTypes";
import type {
  ScheduledTaskActionResult,
  ScheduledTaskInput,
  ScheduledTaskSummary,
} from "../../features/tools/scheduledTaskTypes";
import type {
  LocalPathMetadataResult,
  LocalUploadTempResult,
  RemoteFileDeleteInput,
  RemoteFileDownloadToLocalInput,
  RemoteFileDownloadToLocalResult,
  RemoteFileDownloadResult,
  RemoteFileDownloadTargetCheckInput,
  RemoteFileDownloadTargetCheckResult,
  RemoteFileEntry,
  RemoteFileEntryMetadata,
  RemoteFileArchiveUploadInput,
  RemoteFileArchiveUploadLocalInput,
  RemoteFileArchiveUploadResult,
  RemoteFileMetadata,
  RemoteFilePathCheckResult,
  RemoteFileReadResult,
  RemoteFileRenameInput,
  RemoteFileUploadInput,
  RemoteFileUploadLocalInput,
  RemoteFileUploadResult,
  RemoteFileWriteInput,
  RemoteFileWriteResult,
} from "../../features/files/remoteFileTypes";
import type {
  SerialPortEntry,
  SerialTerminalOpenRequest,
  TelnetTerminalOpenRequest,
} from "../../features/terminal/characterSessionTypes";
import type {
  LocalTerminalOpenRequest,
  LocalTerminalProfile,
  WindowsPtyInfo,
} from "../../features/terminal/localTerminalTypes";
import type {
  RemoteMonitorProcessSignalInput,
  RemoteMonitorSnapshot,
  RemoteMonitorSnapshotOptions,
  RemoteProcessActionResult,
} from "../../features/monitor/monitorTypes";
import type { TerminalConnectRequest } from "../../features/terminal/terminalTypes";
import type {
  TunnelRuleInput,
  TunnelRuleWithState,
  TunnelRuntimeCredentialInput,
} from "../../features/tunnels/tunnelTypes";
import type {
  WebDavDownloadRequest,
  WebDavRemoteInfo,
  WebDavSettings,
  WebDavSettingsInput,
  WebDavSyncResult,
  WebDavTestResult,
  WebDavUploadRequest,
} from "../../features/settings/webdavSyncTypes";
import type {
  McpLocalNetworkInfo,
  McpRemoteLogOutput,
  McpRemoteServiceStatus,
  McpSettings,
  McpUpdateBlockerStatus,
} from "../../features/settings/mcpSettingsTypes";

export interface SecretVaultStatus {
  initialized: boolean;
  unlocked: boolean;
}
export interface NativeWindowMaterial {
  id: number;
  name: string;
}
export type AppDistributionMode =
  | "desktop-installer"
  | "desktop-portable"
  | "desktop-appimage"
  | "desktop-package"
  | "web";

export interface AppRuntimeInfo {
  version: string;
  repositoryUrl: string;
  distributionMode: AppDistributionMode;
  isTauri: boolean;
}

export function secretVaultStatus() {
  return invoke<SecretVaultStatus>("secret_vault_status");
}

export function secretVaultUnlock(masterPassword: string) {
  return invoke<SecretVaultStatus>("secret_vault_unlock", {
    request: {
      master_password: masterPassword,
    },
  });
}

export function secretVaultUnlockLocal() {
  return invoke<SecretVaultStatus>("secret_vault_unlock_local");
}

export function secretVaultLock() {
  return invoke<SecretVaultStatus>("secret_vault_lock");
}

export function secretVaultEnableMasterPassword(masterPassword: string) {
  return invoke<SecretVaultStatus>("secret_vault_enable_master_password", {
    request: {
      master_password: masterPassword,
    },
  });
}

export function secretVaultDisableMasterPassword() {
  return invoke<SecretVaultStatus>("secret_vault_disable_master_password");
}

export function connectionTransferExport(path: string, password: string) {
  return invoke<ConnectionTransferExportResult>("connection_transfer_export", {
    request: { path, password },
  });
}

export function connectionTransferPreview(path: string, password: string) {
  return invoke<ConnectionTransferPreviewResult>("connection_transfer_preview", {
    request: { path, password },
  });
}

export function connectionTransferImport(
  path: string,
  password: string,
  fingerprint: string,
  strategy: ConnectionTransferConflictStrategy,
) {
  return invoke<ConnectionTransferImportResult>("connection_transfer_import", {
    request: { path, password, fingerprint, strategy },
  });
}

export function connectionList() {
  return invoke<ConnectionProfile[]>("connection_list");
}

export function connectionUpsert(request: ConnectionProfileInput) {
  return invoke<ConnectionProfile>("connection_upsert", { request });
}

export function connectionSetFavorite(connectionId: string, isFavorite: boolean) {
  return invoke<ConnectionProfile>("connection_set_favorite", {
    request: {
      connection_id: connectionId,
      is_favorite: isFavorite,
    },
  });
}

export function connectionMarkConnected(connectionId: string) {
  return invoke<ConnectionProfile>("connection_mark_connected", {
    request: {
      connection_id: connectionId,
    },
  });
}

export function connectionDelete(id: string) {
  return invoke<void>("connection_delete", { id });
}

export function connectionRevealInlineSecret(id: string) {
  return invoke<RevealedConnectionSecret>("connection_reveal_inline_secret", { id });
}

export function credentialList() {
  return invoke<CredentialProfile[]>("credential_list");
}

export function credentialUpsert(request: CredentialProfileInput) {
  return invoke<CredentialProfile>("credential_upsert", { request });
}

export function credentialDelete(id: string) {
  return invoke<void>("credential_delete", { id });
}

export function credentialRevealSecret(id: string) {
  return invoke<RevealedCredentialSecret>("credential_reveal_secret", { id });
}

export function commandSnippetList() {
  return invoke<CommandSnippet[]>("command_snippet_list");
}

export function commandSnippetUpsert(request: CommandSnippetInput) {
  return invoke<CommandSnippet>("command_snippet_upsert", { request });
}

export function commandSnippetDelete(id: string) {
  return invoke<void>("command_snippet_delete", {
    request: {
      id,
    },
  });
}

export function commandSnippetMarkUsed(id: string) {
  return invoke<CommandSnippet>("command_snippet_mark_used", {
    request: {
      id,
    },
  });
}

export function commandHistoryList(limit?: number, scope?: CommandHistoryScope | null) {
  return invoke<CommandHistoryEntry[]>("command_history_list", {
    request: {
      limit,
      scope,
    },
  });
}

export function commandHistoryRecord(request: CommandHistoryRecordRequest) {
  return invoke<CommandHistoryEntry>("command_history_record", { request });
}

export function commandHistoryDelete(id: string) {
  return invoke<void>("command_history_delete", {
    request: {
      id,
    },
  });
}

export function commandHistoryClear() {
  return invoke<void>("command_history_clear");
}

export function aiProviderConfigList() {
  return invoke<AiProviderConfig[]>("ai_provider_config_list");
}

export function aiProviderConfigSave(request: AiProviderConfigInput) {
  return invoke<AiProviderConfig>("ai_provider_config_save", { request });
}

export function aiProviderConfigDelete(id: string) {
  return invoke<void>("ai_provider_config_delete", {
    request: {
      id,
    },
  });
}

export function aiProviderConfigRevealApiKey(id: string) {
  return invoke<RevealedAiProviderApiKey>("ai_provider_config_reveal_api_key", {
    request: {
      id,
    },
  });
}

export function aiProviderConfigTest(request: AiProviderConfigInput) {
  return invoke<AiProviderConfigTestResult>("ai_provider_config_test", { request });
}

export function aiProviderModelsList(request: AiProviderConfigInput) {
  return invoke<AiProviderModelOption[]>("ai_provider_models_list", { request });
}

export function aiChatSessionList() {
  return invoke<AiChatSessionSummary[]>("ai_chat_session_list");
}

export function aiChatSessionGet(sessionId: string) {
  return invoke<AiChatSession>("ai_chat_session_get", {
    request: {
      session_id: sessionId,
    },
  });
}

export function aiChatSessionDelete(sessionId: string) {
  return invoke<void>("ai_chat_session_delete", {
    request: {
      session_id: sessionId,
    },
  });
}

export function aiChatSessionClear(sessionId: string) {
  return invoke<AiChatSession>("ai_chat_session_clear", {
    request: {
      session_id: sessionId,
    },
  });
}

export function aiChatStreamStart(request: AiChatStreamStartRequest) {
  return invoke<AiChatStreamStartResponse>("ai_chat_stream_start", { request });
}

export function aiChatStreamStop(streamId: string) {
  return invoke<void>("ai_chat_stream_stop", {
    request: {
      stream_id: streamId,
    },
  });
}

export function aiCommandAssess(command: string) {
  return invoke<AiCommandAssessment>("ai_command_assess", {
    request: {
      command,
    },
  });
}

export function connectionTest(request: ConnectionRuntimeCredentialRequest) {
  return invoke<ConnectionStepResult>("connection_test", { request });
}

export function connectionTestProfile(request: ConnectionProfileInput) {
  return invoke<ConnectionStepResult>("connection_test_profile", { request });
}

export function rdpLaunchConnection(connectionId: string, bounds?: RdpEmbeddedBounds | null) {
  return invoke<RdpLaunchResult>("rdp_launch_connection", {
    request: {
      connection_id: connectionId,
      bounds: bounds ?? undefined,
    },
  });
}

export function rdpPreviewLaunch(connectionId: string) {
  return invoke<RdpLaunchPreview>("rdp_preview_launch", {
    request: {
      connection_id: connectionId,
    },
  });
}

export function rdpTestRunner(config?: RdpRunnerConfig | null) {
  return invoke<RdpRunnerProbeResult>("rdp_test_runner", {
    request: {
      config,
    },
  });
}

export function rdpCloseSession(sessionId: string) {
  return invoke<RdpSessionCloseResult>("rdp_close_session", {
    request: {
      session_id: sessionId,
    },
  });
}

export function rdpRevealSession(sessionId: string) {
  return invoke<RdpSessionRevealResult>("rdp_reveal_session", {
    request: {
      session_id: sessionId,
    },
  });
}

export function rdpResizeEmbeddedSession(
  sessionId: string,
  bounds: { x: number; y: number; width: number; height: number },
) {
  return invoke<RdpSessionResizeResult>("rdp_resize_embedded_session", {
    request: {
      session_id: sessionId,
      bounds,
    },
  });
}

export function vncLaunchConnection(connectionId: string) {
  return invoke<VncLaunchResult>("vnc_launch_connection", {
    request: {
      connection_id: connectionId,
    },
  });
}

export function vncPreviewLaunch(connectionId: string) {
  return invoke<VncLaunchPreview>("vnc_preview_launch", {
    request: {
      connection_id: connectionId,
    },
  });
}

export function vncTestRunner(config?: VncRunnerConfig | null) {
  return invoke<VncRunnerProbeResult>("vnc_test_runner", {
    request: {
      config,
    },
  });
}

export function vncCloseSession(sessionId: string) {
  return invoke<VncSessionCloseResult>("vnc_close_session", {
    request: {
      session_id: sessionId,
    },
  });
}

export function connectionProbeSystem(request: ConnectionRuntimeCredentialRequest) {
  return invoke<ConnectionProfile>("connection_probe_system", { request });
}

export function knownHostTrust(hostKey: HostKeyInfo) {
  return invoke<void>("known_host_trust", {
    request: {
      host_key: hostKey,
    },
  });
}

export function connectionProbeLatency(connectionId: string) {
  return invoke<{ latency_ms: number | null; reachable: boolean }>("connection_probe_latency", {
    request: {
      connection_id: connectionId,
    },
  });
}

export function terminalConnect(request: TerminalConnectRequest) {
  return invoke<string>("terminal_connect", { request });
}

export function terminalWrite(sessionId: string, data: string) {
  return invoke<void>("terminal_write", {
    request: {
      data,
      session_id: sessionId,
    },
  });
}

export function terminalResize(sessionId: string, cols: number, rows: number) {
  return invoke<void>("terminal_resize", {
    request: {
      cols,
      rows,
      session_id: sessionId,
    },
  });
}

export function terminalClose(sessionId: string) {
  return invoke<void>("terminal_close", { sessionId });
}

export function localTerminalListProfiles(input?: {
  platform?: string;
  hiddenProfileIds?: string[];
}) {
  return invoke<LocalTerminalProfile[]>("local_terminal_list_profiles", {
    request: {
      hidden_profile_ids: input?.hiddenProfileIds || [],
      platform: input?.platform,
    },
  });
}

export function localTerminalOpen(request: LocalTerminalOpenRequest) {
  return invoke<string>("local_terminal_open", { request });
}

export function telnetTerminalOpen(request: TelnetTerminalOpenRequest) {
  return invoke<string>("telnet_terminal_open", { request });
}

export function serialListPorts() {
  return invoke<SerialPortEntry[]>("serial_list_ports");
}

export function serialTerminalOpen(request: SerialTerminalOpenRequest) {
  return invoke<string>("serial_terminal_open", { request });
}

export function getWindowsPtyInfo() {
  return invoke<WindowsPtyInfo | null>("get_windows_pty_info");
}

export function getAppRuntimeInfoCommand() {
  return invoke<AppRuntimeInfo>("get_app_runtime_info");
}

export function getSupportedWindowMaterialsCommand() {
  return invoke<NativeWindowMaterial[]>("get_supported_window_materials");
}

export function setWindowMaterialCommand(material: number) {
  return invoke<NativeWindowMaterial>("set_window_material", {
    material,
  });
}

export function remoteFileList(connectionId: string, path?: string) {
  return invoke<RemoteFileEntry[]>("remote_file_list", {
    request: {
      connection_id: connectionId,
      path,
    },
  });
}

export function remoteMonitorSnapshot(
  connectionId: string,
  options: RemoteMonitorSnapshotOptions = {},
) {
  return invoke<RemoteMonitorSnapshot>("remote_monitor_snapshot", {
    request: {
      connection_id: connectionId,
      include_processes: options.includeProcesses ?? false,
      process_limit: options.processLimit,
    },
  });
}

export function remoteMonitorProcessSignal({
  connectionId,
  pid,
  signal,
}: RemoteMonitorProcessSignalInput) {
  return invoke<RemoteProcessActionResult>("remote_monitor_process_signal", {
    request: {
      connection_id: connectionId,
      pid,
      signal,
    },
  });
}

export function dockerListContainers(connectionId: string) {
  return invoke<DockerContainerSummary[]>("docker_list_containers", {
    request: {
      connection_id: connectionId,
    },
  });
}

export function dockerListImages(connectionId: string) {
  return invoke<DockerImageSummary[]>("docker_list_images", {
    request: {
      connection_id: connectionId,
    },
  });
}

export function dockerContainerAction(
  connectionId: string,
  containerId: string,
  action: DockerContainerAction,
) {
  return invoke<DockerActionResult>("docker_container_action", {
    request: {
      action,
      connection_id: connectionId,
      container_id: containerId,
    },
  });
}

export function dockerContainerLogs(connectionId: string, containerId: string, tail = 120) {
  return invoke<DockerLogsResult>("docker_container_logs", {
    request: {
      connection_id: connectionId,
      container_id: containerId,
      tail,
    },
  });
}

export function dockerContainerInspect(connectionId: string, containerId: string) {
  return invoke<DockerContainerDetail>("docker_container_inspect", {
    request: {
      connection_id: connectionId,
      container_id: containerId,
    },
  });
}

export function dockerContainerUpdateRestartPolicy(
  connectionId: string,
  containerId: string,
  policy: DockerRestartPolicyKind,
) {
  return invoke<DockerActionResult>("docker_container_update_restart_policy", {
    request: {
      connection_id: connectionId,
      container_id: containerId,
      policy,
    },
  });
}

export function dockerListNetworks(connectionId: string) {
  return invoke<DockerNetworkSummary[]>("docker_list_networks", {
    request: {
      connection_id: connectionId,
    },
  });
}

export function dockerContainerConnectNetwork(
  connectionId: string,
  containerId: string,
  networkId: string,
) {
  return invoke<DockerActionResult>("docker_container_connect_network", {
    request: {
      connection_id: connectionId,
      container_id: containerId,
      network_id: networkId,
    },
  });
}

export function dockerContainerLogsStart(
  connectionId: string,
  containerId: string,
  streamId: string,
  tail = 300,
) {
  return invoke<void>("docker_container_logs_start", {
    request: {
      connection_id: connectionId,
      container_id: containerId,
      stream_id: streamId,
      tail,
    },
  });
}

export function dockerContainerLogsStop(streamId: string) {
  return invoke<void>("docker_container_logs_stop", {
    request: {
      stream_id: streamId,
    },
  });
}

export function dockerContainerLogsSave(localPath: string, content: string) {
  return invoke<void>("docker_container_logs_save", {
    request: {
      content,
      local_path: localPath,
    },
  });
}

export function dockerImagePull(connectionId: string, image: string, pullId?: string) {
  return invoke<DockerActionResult>("docker_image_pull", {
    request: {
      connection_id: connectionId,
      image,
      pull_id: pullId,
    },
  });
}

export function dockerImageRemove(connectionId: string, imageId: string) {
  return invoke<DockerActionResult>("docker_image_remove", {
    request: {
      connection_id: connectionId,
      image_id: imageId,
    },
  });
}

export function dockerImageRun(connectionId: string, request: DockerImageRunRequest) {
  return invoke<DockerActionResult>("docker_image_run", {
    request: {
      ...request,
      connection_id: connectionId,
    },
  });
}

export function dockerEngineStatus(connectionId: string) {
  return invoke<DockerEngineStatus>("docker_engine_status", {
    request: {
      connection_id: connectionId,
    },
  });
}

export function dockerEngineAction(connectionId: string, action: DockerEngineAction) {
  return invoke<DockerActionResult>("docker_engine_action", {
    request: {
      action,
      connection_id: connectionId,
    },
  });
}

export function dockerEngineReadConfig(connectionId: string) {
  return invoke<DockerEngineConfigResult>("docker_engine_read_config", {
    request: {
      connection_id: connectionId,
    },
  });
}

export function dockerEngineSaveConfig(connectionId: string, content: string) {
  return invoke<DockerActionResult>("docker_engine_save_config", {
    request: {
      connection_id: connectionId,
      content,
    },
  });
}

export function networkDiagnosticRun(
  connectionId: string,
  request: NetworkDiagnosticRequest,
) {
  return invoke<NetworkDiagnosticResult>("network_diagnostic_run", {
    request: {
      ...request,
      connection_id: connectionId,
    },
  });
}

export function scheduledTaskList(connectionId: string) {
  return invoke<ScheduledTaskSummary[]>("scheduled_task_list", {
    request: {
      connection_id: connectionId,
    },
  });
}

export function scheduledTaskSave(connectionId: string, task: ScheduledTaskInput) {
  return invoke<ScheduledTaskSummary>("scheduled_task_save", {
    request: {
      connection_id: connectionId,
      task,
    },
  });
}

export function scheduledTaskDelete(connectionId: string, taskId: string) {
  return invoke<ScheduledTaskActionResult>("scheduled_task_delete", {
    request: {
      connection_id: connectionId,
      task_id: taskId,
    },
  });
}

export function scheduledTaskSetEnabled(
  connectionId: string,
  taskId: string,
  enabled: boolean,
) {
  return invoke<ScheduledTaskSummary>("scheduled_task_set_enabled", {
    request: {
      connection_id: connectionId,
      task_id: taskId,
      enabled,
    },
  });
}

export function scheduledTaskRunNow(connectionId: string, taskId: string) {
  return invoke<ScheduledTaskActionResult>("scheduled_task_run_now", {
    request: {
      connection_id: connectionId,
      task_id: taskId,
    },
  });
}

export function dockerExecInvalidateConnection(connectionId: string) {
  return invoke<void>("docker_exec_invalidate_connection", {
    request: {
      connection_id: connectionId,
    },
  });
}

export function remoteFileRead(connectionId: string, path: string) {
  return invoke<RemoteFileReadResult>("remote_file_read", {
    request: {
      connection_id: connectionId,
      path,
    },
  });
}

export function remoteFileWrite({
  connectionId,
  content,
  expectedMtime,
  expectedSize,
  overwrite = false,
  path,
}: RemoteFileWriteInput) {
  return invoke<RemoteFileWriteResult>("remote_file_write", {
    request: {
      connection_id: connectionId,
      content,
      expected_mtime: expectedMtime,
      expected_size: expectedSize,
      overwrite,
      path,
    },
  });
}

export function remoteFileCreateFile(connectionId: string, path: string) {
  return invoke<RemoteFileMetadata>("remote_file_create_file", {
    request: {
      connection_id: connectionId,
      path,
    },
  });
}

export function remoteFileCreateDirectory(connectionId: string, path: string) {
  return invoke<void>("remote_file_create_directory", {
    request: {
      connection_id: connectionId,
      path,
    },
  });
}

export function remoteFileRename({ connectionId, newPath, path }: RemoteFileRenameInput) {
  return invoke<void>("remote_file_rename", {
    request: {
      connection_id: connectionId,
      new_path: newPath,
      path,
    },
  });
}

export function remoteFileDelete({ connectionId, path, recursive = false }: RemoteFileDeleteInput) {
  return invoke<void>("remote_file_delete", {
    request: {
      connection_id: connectionId,
      path,
      recursive,
    },
  });
}

export function remoteFileMetadata(connectionId: string, path: string) {
  return invoke<RemoteFileEntryMetadata>("remote_file_metadata", {
    request: {
      connection_id: connectionId,
      path,
    },
  });
}

export function remoteFileCheckPath(connectionId: string, path: string) {
  return invoke<RemoteFilePathCheckResult>("remote_file_check_path", {
    request: {
      connection_id: connectionId,
      path,
    },
  });
}

export function remoteFileUploadFile({
  connectionId,
  content,
  conflictPolicy = "rename",
  path,
  transferId,
}: RemoteFileUploadInput) {
  return invoke<RemoteFileUploadResult>("remote_file_upload_file", {
    request: {
      connection_id: connectionId,
      content: Array.from(content),
      conflict_policy: conflictPolicy,
      path,
      transfer_id: transferId,
    },
  });
}

export function remoteFileUploadLocalFile({
  connectionId,
  conflictPolicy = "rename",
  localPath,
  path,
  transferId,
}: RemoteFileUploadLocalInput) {
  return invoke<RemoteFileUploadResult>("remote_file_upload_local_file", {
    request: {
      connection_id: connectionId,
      conflict_policy: conflictPolicy,
      local_path: localPath,
      path,
      transfer_id: transferId,
    },
  });
}

export function remoteFileUploadArchive({
  archiveContent,
  connectionId,
  conflictPolicy = "rename",
  keepArchive = false,
  rootName,
  targetDir,
  transferId,
}: RemoteFileArchiveUploadInput) {
  return invoke<RemoteFileArchiveUploadResult>("remote_file_upload_archive", {
    request: {
      archive_content: Array.from(archiveContent),
      connection_id: connectionId,
      conflict_policy: conflictPolicy,
      keep_archive: keepArchive,
      root_name: rootName,
      target_dir: targetDir,
      transfer_id: transferId,
    },
  });
}

export function remoteFileUploadLocalArchive({
  compress = true,
  connectionId,
  conflictPolicy = "rename",
  keepArchive = false,
  localPath,
  rootName,
  targetDir,
  transferId,
}: RemoteFileArchiveUploadLocalInput) {
  return invoke<RemoteFileArchiveUploadResult>("remote_file_upload_local_archive", {
    request: {
      compress,
      connection_id: connectionId,
      conflict_policy: conflictPolicy,
      keep_archive: keepArchive,
      local_path: localPath,
      root_name: rootName,
      target_dir: targetDir,
      transfer_id: transferId,
    },
  });
}

export function remoteFilePrepareUploadTemp(fileName: string) {
  return invoke<LocalUploadTempResult>("remote_file_prepare_upload_temp", {
    request: {
      file_name: fileName,
    },
  });
}

export function remoteFileAppendUploadTemp(localPath: string, chunk: Uint8Array | number[]) {
  return invoke<void>("remote_file_append_upload_temp", {
    request: {
      chunk,
      local_path: localPath,
    },
  });
}

export function remoteFileDeleteUploadTemp(localPath: string) {
  return invoke<void>("remote_file_delete_upload_temp", {
    request: {
      local_path: localPath,
    },
  });
}

export function localPathMetadata(path: string) {
  return invoke<LocalPathMetadataResult>("local_path_metadata", {
    request: {
      path,
    },
  });
}

export function remoteFileDownload(connectionId: string, path: string) {
  return invoke<RemoteFileDownloadResult>("remote_file_download", {
    request: {
      connection_id: connectionId,
      path,
    },
  });
}

export function remoteFileCheckDownloadTarget({
  connectionId,
  directory = false,
  downloadRoot,
  groupBySession = true,
  path,
  sessionName,
  timestampDirectory = true,
  timestampName,
}: RemoteFileDownloadTargetCheckInput) {
  return invoke<RemoteFileDownloadTargetCheckResult>("remote_file_check_download_target", {
    request: {
      connection_id: connectionId,
      directory,
      download_root: downloadRoot,
      group_by_session: groupBySession,
      path,
      session_name: sessionName,
      timestamp_directory: timestampDirectory,
      timestamp_name: timestampName,
    },
  });
}

export function remoteFileDownloadToLocal({
  compress = true,
  connectionId,
  conflictPolicy = "rename",
  directory = false,
  downloadRoot,
  groupBySession = true,
  keepArchives = false,
  path,
  sessionName,
  timestampDirectory = true,
  timestampName,
  transferId,
}: RemoteFileDownloadToLocalInput) {
  return invoke<RemoteFileDownloadToLocalResult>("remote_file_download_to_local", {
    request: {
      compress,
      connection_id: connectionId,
      conflict_policy: conflictPolicy,
      directory,
      download_root: downloadRoot,
      group_by_session: groupBySession,
      keep_archives: keepArchives,
      path,
      session_name: sessionName,
      timestamp_directory: timestampDirectory,
      timestamp_name: timestampName,
      transfer_id: transferId,
    },
  });
}

export function remoteFileCancelTransfer(transferId: string) {
  return invoke<boolean>("remote_file_cancel_transfer", {
    request: {
      transfer_id: transferId,
    },
  });
}
export function tunnelList() {
  return invoke<TunnelRuleWithState[]>("tunnel_list");
}

export function tunnelUpsert(request: TunnelRuleInput) {
  return invoke<TunnelRuleWithState>("tunnel_upsert", { request });
}

export function tunnelDelete(ruleId: string) {
  return invoke<void>("tunnel_delete", {
    request: {
      rule_id: ruleId,
    },
  });
}

export function tunnelStart(ruleId: string, runtimeCredential?: TunnelRuntimeCredentialInput) {
  return invoke<TunnelRuleWithState>("tunnel_start", {
    request: {
      rule_id: ruleId,
      runtime_credential: runtimeCredential,
    },
  });
}

export function tunnelStop(ruleId: string) {
  return invoke<TunnelRuleWithState>("tunnel_stop", {
    request: {
      rule_id: ruleId,
    },
  });
}

export function tunnelAutostart() {
  return invoke<TunnelRuleWithState[]>("tunnel_autostart");
}

export function webdavSettingsGet() {
  return invoke<WebDavSettings>("webdav_settings_get");
}

export function webdavSettingsSave(request: WebDavSettingsInput) {
  return invoke<WebDavSettings>("webdav_settings_save", { request });
}

export function webdavTestConnection(request?: WebDavSettingsInput) {
  return invoke<WebDavTestResult>("webdav_test_connection", {
    request: request ?? null,
  });
}

export function webdavFetchRemoteInfo() {
  return invoke<WebDavRemoteInfo>("webdav_fetch_remote_info");
}

export function webdavUploadSnapshot(request: WebDavUploadRequest) {
  return invoke<WebDavSyncResult>("webdav_upload_snapshot", { request });
}

export function webdavDownloadSnapshot(request: WebDavDownloadRequest) {
  return invoke<WebDavSyncResult>("webdav_download_snapshot", { request });
}

export function mcpSettingsGet() {
  return invoke<McpSettings>("mcp_settings_get");
}

export function mcpSettingsSave(request: McpSettings) {
  return invoke<McpSettings>("mcp_settings_save", { request });
}

export function mcpExecutablePath() {
  return invoke<string>("mcp_executable_path");
}

export function mcpLocalNetworkInfo() {
  return invoke<McpLocalNetworkInfo>("mcp_local_network_info");
}

export function mcpRemoteServiceStatus() {
  return invoke<McpRemoteServiceStatus>("mcp_remote_service_status");
}

export function mcpRemoteServiceStart() {
  return invoke<McpRemoteServiceStatus>("mcp_remote_service_start");
}

export function mcpRemoteServiceStop() {
  return invoke<McpRemoteServiceStatus>("mcp_remote_service_stop");
}

export function mcpRemoteServiceRestart() {
  return invoke<McpRemoteServiceStatus>("mcp_remote_service_restart");
}

export function mcpUpdateBlockers() {
  return invoke<McpUpdateBlockerStatus>("mcp_update_blockers");
}

export function mcpPrepareForUpdate() {
  return invoke<McpUpdateBlockerStatus>("mcp_prepare_for_update");
}

export function mcpRemoteLogRead() {
  return invoke<McpRemoteLogOutput>("mcp_remote_log_read");
}

export function mcpRemoteLogClear() {
  return invoke<McpRemoteLogOutput>("mcp_remote_log_clear");
}

export function mcpRemoteTokenRotate() {
  return invoke<McpSettings>("mcp_remote_token_rotate");
}


// ---- Database workspace (navop parity T018+) ----
export function dbEngineList() {
  return invoke<DbEngineInfo[]>("db_engine_list");
}

export function dbConnectionList() {
  return invoke<DbConnectionProfile[]>("db_connection_list");
}

export function dbConnectionSave(input: DbConnectionInput) {
  return invoke<DbConnectionProfile>("db_connection_save", { request: input });
}

export function dbConnectionDelete(connectionId: string) {
  return invoke<void>("db_connection_delete", { request: { connection_id: connectionId } });
}

export function dbConnectionTest(input: DbConnectionInput) {
  return invoke<DbTestResult>("db_connection_test", { request: input });
}

export function dbConnectionConnect(input: DbConnectionInput) {
  return invoke<DbConnectResult>("db_connection_connect", { request: input });
}

export function dbConnectionDisconnect(sessionId: string) {
  return invoke<boolean>("db_connection_disconnect", { request: { session_id: sessionId } });
}

export function dbExport(request: { session_id: string; sql: string; format: string; table?: string }) {
  return invoke<{ format: string; rows: number; chars: number; path: string | null; content: string }>("db_export", { request });
}

export function dbImport(request: { session_id: string; table: string; format: string; content: string }) {
  return invoke<{ format: string; statements: number; imported: boolean }>("db_import", { request });
}

export function dbCompare(sourceSessionId: string, targetSessionId: string, mode = "all") {
  return invoke<DbCompareResult>("db_compare", {
    request: { source_session_id: sourceSessionId, target_session_id: targetSessionId, mode },
  });
}

export function dbErMetadata(sessionId: string) {
  return invoke<DbErMetadata>("db_er_metadata", { request: { session_id: sessionId } });
}

export function dbProxyRoute(request: DbConnectionInput) {
  return invoke<DbProxyRoute>("db_proxy_route", { request });
}

export function redisKeys(sessionId: string, pattern = "*") {
  return invoke<RedisKeyList>("redis_keys", { request: { session_id: sessionId, pattern } });
}

export function redisGet(sessionId: string, key: string) {
  return invoke<RedisKeyValue>("redis_get", { request: { session_id: sessionId, key } });
}

export function redisSet(sessionId: string, key: string, value: string, ttlSeconds?: number) {
  return invoke<RedisSetResult>("redis_set", {
    request: { session_id: sessionId, key, value, ttl_seconds: ttlSeconds },
  });
}

export function redisTtl(sessionId: string, key: string) {
  return invoke<RedisTtlResult>("redis_ttl", { request: { session_id: sessionId, key } });
}

export function redisDel(sessionId: string, key: string) {
  return invoke<RedisDelResult>("redis_del", { request: { session_id: sessionId, key } });
}

export function redisType(sessionId: string, key: string) {
  return invoke<RedisTypeResult>("redis_type", { request: { session_id: sessionId, key } });
}

export function redisConsole(sessionId: string, command: string) {
  return invoke<RedisConsoleResult>("redis_console", {
    request: { session_id: sessionId, command },
  });
}

export function mongodbCollections(sessionId: string) {
  return invoke<MongoCollectionsResult>("mongodb_collections", { request: { session_id: sessionId } });
}

export function mongodbDocuments(sessionId: string, collection: string, filter = "{}", limit = 50) {
  return invoke<MongoDocumentsResult>("mongodb_documents", {
    request: { session_id: sessionId, collection, filter, limit },
  });
}

export function mongodbInsert(sessionId: string, collection: string, document: string) {
  return invoke<MongoInsertResult>("mongodb_insert", {
    request: { session_id: sessionId, collection, document },
  });
}

export function mongodbUpdate(sessionId: string, collection: string, filter: string, update: string) {
  return invoke<MongoUpdateResult>("mongodb_update", {
    request: { session_id: sessionId, collection, filter, update },
  });
}

export function mongodbDelete(sessionId: string, collection: string, filter: string) {
  return invoke<MongoDeleteResult>("mongodb_delete", {
    request: { session_id: sessionId, collection, filter },
  });
}

export function notesDir() {
  return invoke<NotesDirResult>("notes_dir");
}

export function notesList() {
  return invoke<NotesListResult>("notes_list");
}

export function notesRead(name: string) {
  return invoke<NoteReadResult>("notes_read", { request: { name } });
}

export function notesSave(name: string, content: string) {
  return invoke<NoteSaveResult>("notes_save", { request: { name, content } });
}

export function notesDelete(name: string) {
  return invoke<NoteDeleteResult>("notes_delete", { request: { name } });
}

export function notesAsset(relative: string) {
  return invoke<NoteAssetResult>("notes_asset", { request: { relative } });
}

export function notesExport(name: string, format: string, html: string, text: string) {
  return invoke<NoteExportResult>("notes_export", {
    request: { name, format, html, text },
  });
}

export interface TerminalBroadcastResult {
  enabled?: boolean;
  input?: string;
  targets?: number;
  results?: { session_id: string; ok: boolean }[];
}

export function terminalSetBroadcast(enabled: boolean) {
  return invoke<TerminalBroadcastResult>("terminal_set_broadcast", { request: { enabled } });
}

export function terminalBroadcastStatus() {
  return invoke<TerminalBroadcastResult>("terminal_broadcast_status");
}

export function terminalBroadcast(input: string, exclude?: string) {
  return invoke<TerminalBroadcastResult>("terminal_broadcast", {
    request: { input, exclude },
  });
}

export interface RemoteFileCopyResult {
  source: string;
  target: string;
  copied_bytes: number;
}

export function remoteFileCopyAcross(
  source: Record<string, unknown>,
  sourcePath: string,
  target: Record<string, unknown>,
  targetPath: string,
) {
  return invoke<RemoteFileCopyResult>("remote_file_copy_across", {
    request: { source, source_path: sourcePath, target, target_path: targetPath },
  });
}

export interface CommandSendToTerminalResult {
  session_id: string;
  sent: boolean;
  recorded: boolean;
}

export function commandSendToTerminal(sessionId: string, command: string) {
  return invoke<CommandSendToTerminalResult>("command_send_to_terminal", {
    request: { session_id: sessionId, command },
  });
}

export function agentList() {
  return invoke<AgentInfo[]>("agent_list");
}

export function agentStart(kind: string) {
  return invoke<AgentStartResult>("agent_start", { request: { kind } });
}

export function agentStop(sessionId: string) {
  return invoke<AgentStopResult>("agent_stop", { request: { session_id: sessionId } });
}

export function agentProjectFiles(path?: string, relative?: string) {
  return invoke<AgentProjectFilesResult>("agent_project_files", { request: { path, relative } });
}

export function gitBranches(repo: string) {
  return invoke<GitBranchesResult>("git_branches", { request: { repo } });
}

export function gitStatus(repo: string) {
  return invoke<GitStatusResult>("git_status", { request: { repo } });
}

export function gitDiff(repo: string, file: string) {
  return invoke<GitDiffResult>("git_diff", { request: { repo, file } });
}

export function gitSwitch(repo: string, branch: string) {
  return invoke<GitSwitchResult>("git_switch", { request: { repo, branch } });
}

export interface AiPromptResult {
  role: string;
  prompt: string;
}

export interface AiChartSeries {
  name: string;
  values: number[];
}

export interface AiChartSpec {
  chart_type: string;
  reason?: string;
  category?: string;
  labels?: string[];
  series?: AiChartSeries[];
}

export function aiExplainPrompt(sql: string, dialect?: string) {
  return invoke<AiPromptResult>("ai_explain_prompt", { request: { sql, dialect } });
}

export function aiGenerateSqlPrompt(text: string, dialect?: string, schema?: string) {
  return invoke<AiPromptResult>("ai_generate_sql_prompt", { request: { text, dialect, schema } });
}

export function aiChartSpec(sessionId: string, sql: string) {
  return invoke<AiChartSpec>("ai_chart_spec", { request: { session_id: sessionId, sql } });
}

export function aiChartSpecData(columns: string[], rows: unknown[][]) {
  return invoke<AiChartSpec>("ai_chart_spec_data", { request: { columns, rows } });
}

export function dbObjectList(sessionId: string, kind?: string) {
  return invoke<DbObjectInfo[]>("db_object_list", { request: { session_id: sessionId, kind } });
}

export function dbExplain(sessionId: string, sql: string) {
  return invoke<DbQueryResult>("db_explain", { request: { session_id: sessionId, sql } });
}

export function dbQuery(request: DbQueryRequest) {
  return invoke<DbQueryResult>("db_query", { request });
}

export function dbExec(request: DbQueryRequest) {
  return invoke<DbQueryResult>("db_exec", { request });
}
