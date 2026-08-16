export interface AcpAgentInfo {
  binary: string;
  label: string;
  available: boolean;
}

export interface AcpHandshakeResult {
  binary: string;
  agent: string;
  protocol_version: string;
  handshake: string;
}

export interface AcpRunToolResult {
  binary: string;
  tool: string;
  response: unknown;
}