export interface GitBranch {
  name: string;
  current: boolean;
}

export interface GitBranchesResult {
  branches: GitBranch[];
  current: string;
}

export interface GitStatusEntry {
  path: string;
  status: string;
}

export interface GitStatusResult {
  entries: GitStatusEntry[];
}

export interface GitDiffLine {
  type: string;
  old_line: number | null;
  new_line: number | null;
  text: string;
}

export interface GitDiffHunk {
  old_start: number;
  new_start: number;
  lines: GitDiffLine[];
}

export interface GitDiffResult {
  file: string;
  hunks: GitDiffHunk[];
  raw: string;
}

export interface GitSwitchResult {
  branch: string;
  output: string;
}