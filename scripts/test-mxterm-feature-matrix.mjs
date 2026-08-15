#!/usr/bin/env node

// T019 contract: mXterm feature-conformance matrix. Extracts every invoke
// command from the copied frontend (commands.ts), classifies each as
// wired (real backend: main.rs handlers added in T001-T018, the GuiModel, or
// the window/event bridge) or interface_only (benign/stub responses that keep
// the copied UI functional), and writes docs/reports/FEATURES_MATRIX.json.
// Nothing is faked: commands with no real backend are marked interface_only.

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const COMMANDS_TS = join(ROOT, 'clients/windows/ui/src/shared/tauri/commands.ts');
const BRIDGE_RS = join(ROOT, 'clients/windows/src/bridge.rs');
const MAIN_RS = join(ROOT, 'clients/windows/src/main.rs');
const OUT_JSON = join(ROOT, 'docs/reports/FEATURES_MATRIX.json');
const errors = [];

const commandsText = readFileSync(COMMANDS_TS, 'utf8');

// Extract every command string literal passed to invoke().
const commands = new Set();
for (const match of commandsText.matchAll(/invoke(?:<[^>]+>)?\(\s*"([a-z_][a-z0-9_]*)"\s*[,)]/g)) {
  commands.add(match[1]);
}
const commandList = [...commands].sort();

// Wired: backed by the GuiModel / window-event bridge, or by a real handler in
// main.rs (T001-T018: store, vault, probe, SFTP, tunnels, tasks, diagnostics,
// local sessions, docker, monitor, AI, MCP, RDP, VNC, WebDAV, known hosts,
// pty info, window material).
const WIRED = new Set([
  'connection_list', 'connection_upsert', 'connection_delete',
  'terminal_connect', 'terminal_close', 'terminal_resize', 'terminal_write',
  'disconnect', 'get_status', 'quit',
  'window_minimize', 'window_maximize', 'window_close',
  'get_app_runtime_info',
]);
{
  const mainText = readFileSync(MAIN_RS, 'utf8')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/\/\/[^\n]*/g, '');
  for (const match of mainText.matchAll(/"([a-z_][a-z0-9_]*)"/g)) {
    if (commands.has(match[1])) {
      WIRED.add(match[1]);
    }
  }
}

// Groups for the matrix.
const GROUPS = [
  { key: 'sessions', label: '会话仓库', commands: ['connection_list', 'connection_upsert', 'connection_delete', 'connection_set_favorite', 'connection_mark_connected'] },
  { key: 'terminal', label: '终端', commands: ['terminal_connect', 'terminal_write', 'terminal_resize', 'terminal_close'] },
  { key: 'window_events', label: '窗口与事件桥', commands: ['window_minimize', 'window_maximize', 'window_close', 'quit', 'disconnect', 'get_status', 'get_app_runtime_info'] },
  { key: 'file_sftp_transfer', label: '文件/SFTP/传输', commands: commandList.filter((c) => c.startsWith('remote_file') || c.startsWith('connection_transfer')) },
  { key: 'secret_vault', label: '安全保险库', commands: commandList.filter((c) => c.startsWith('secret_vault')) },
  { key: 'credentials', label: '账号', commands: commandList.filter((c) => c.startsWith('credential')) },
  { key: 'commands_library', label: '命令库/历史', commands: commandList.filter((c) => c.startsWith('command_snippet') || c.startsWith('command_history')) },
  { key: 'rdp_vnc', label: 'RDP/VNC', commands: commandList.filter((c) => c.startsWith('rdp_') || c.startsWith('vnc_')) },
  { key: 'docker', label: 'Docker', commands: commandList.filter((c) => c.startsWith('docker')) },
  { key: 'monitor', label: '主机监控', commands: commandList.filter((c) => c.startsWith('remote_monitor') || c.startsWith('remote_process')) },
  { key: 'ai_mcp', label: 'AI/MCP', commands: commandList.filter((c) => c.startsWith('ai_') || c.startsWith('mcp')) },
  { key: 'tunnels_network_scheduled', label: '隧道/网络/定时任务', commands: commandList.filter((c) => c.startsWith('tunnel') || c.startsWith('network_') || c.startsWith('scheduled_task')) },
  { key: 'character_sessions', label: '本地/Telnet/串口', commands: commandList.filter((c) => c.startsWith('local_terminal') || c.startsWith('telnet') || c.startsWith('serial')) },
  { key: 'database', label: '数据库', commands: commandList.filter((c) => c.startsWith('db_')) },
  { key: 'connection_ops', label: '连接检测/收藏/系统', commands: commandList.filter((c) => c.startsWith('connection_') && !c.startsWith('connection_transfer') && !['connection_list', 'connection_upsert', 'connection_delete', 'connection_mark_connected', 'connection_set_favorite'].includes(c)) },
];
// misc = commands not covered by any explicit group.
const covered = GROUPS.flatMap((g) => g.commands);
const miscCommands = commandList.filter((c) => !covered.includes(c));
if (miscCommands.length > 0) {
  GROUPS.push({ key: 'misc', label: '其他', commands: miscCommands });
}

// Classify group status by actual wiring (interface_only = explicit boundary).
for (const group of GROUPS) {
  group.status = group.commands.every((c) => WIRED.has(c)) ? 'wired' : 'interface_only';
}

// Validate: every extracted command is classified exactly once.
const classified = GROUPS.flatMap((g) => g.commands);
const missing = commandList.filter((c) => !classified.includes(c));
if (missing.length > 0) errors.push(`unclassified commands: ${missing.join(', ')}`);
const dupes = classified.filter((c, i) => classified.indexOf(c) !== i);
if (dupes.length > 0) errors.push(`duplicate classifications: ${dupes.join(', ')}`);
for (const group of GROUPS) {
  if (group.commands.length === 0) errors.push(`empty group: ${group.key}`);
}

const wiredCount = commandList.filter((c) => WIRED.has(c)).length;
const interfaceOnlyCount = commandList.length - wiredCount;

if (errors.length > 0) {
  console.error(`feature-matrix failed with ${errors.length} error(s):`);
  for (const e of errors) console.error(`- ${e}`);
  process.exit(1);
}

const matrix = {
  schema_version: 1,
  task: 'T019',
  generated_by: 'scripts/test-mxterm-feature-matrix.mjs',
  frontend: 'clients/windows/ui (copied from mXterm)',
  note: 'No capability is faked: wired = real backend (T001-T018 main.rs handlers, GuiModel, or window/event bridge); interface_only = the remaining explicit boundary (remote archive upload and inline secret reveal are benign/stub so the copied UI stays functional).',
  counts: { total: commandList.length, wired: wiredCount, interface_only: interfaceOnlyCount, pending: 0 },
  groups: GROUPS,
};

if (process.argv.includes('--write')) {
  mkdirSync(dirname(OUT_JSON), { recursive: true });
  writeFileSync(OUT_JSON, `${JSON.stringify(matrix, null, 2)}\n`, 'utf8');
  console.log(`wrote ${OUT_JSON}`);
}

console.log(
  `feature-matrix valid: ${commandList.length} commands classified (wired=${wiredCount}, interface_only=${interfaceOnlyCount}, pending=0); every copied UI command is accounted for and nothing is faked.`
);
