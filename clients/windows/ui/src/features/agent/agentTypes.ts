export interface AgentInfo {
  id: string;
  kind: string;
  name: string;
  status: string;
  terminal: boolean;
}

export interface AgentStartResult {
  id: string;
  kind: string;
  status: string;
}

export interface AgentStopResult {
  id: string;
  stopped: boolean;
}

export interface AgentProjectEntry {
  name: string;
  relative: string;
  type: string;
  size: number;
}

export interface AgentProjectFilesResult {
  root: string;
  relative: string;
  entries: AgentProjectEntry[];
}