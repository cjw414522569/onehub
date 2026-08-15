// Offline connectivity policy (T140): when the browser is offline the shell
// must never claim a live connection. Sessions are suspended (shown as
// offline / not connected) and reconnects are only offered while online.

/** Browser connectivity. */
export type Connectivity = 'online' | 'offline';

/** The offline / connectivity policy. */
export class OfflinePolicy {
  /** Whether a connection may be attempted. */
  canConnect(connectivity: Connectivity): boolean {
    return connectivity === 'online';
  }

  /**
   * Status text for the shell. Offline never claims connectivity: it reads
   * "Offline - not connected" regardless of the underlying phase.
   */
  statusText(connectivity: Connectivity, phase: string): string {
    if (connectivity === 'offline') return 'Offline - not connected';
    switch (phase) {
      case 'ready':
        return 'Connected - encrypted session';
      case 'connecting':
        return 'Connecting...';
      case 'offline':
        return 'Reconnecting...';
      default:
        return 'Disconnected';
    }
  }
}