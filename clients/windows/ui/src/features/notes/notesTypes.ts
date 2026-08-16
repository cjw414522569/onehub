export interface NotesDirResult {
  dir: string;
}

export interface NotesListResult {
  notes: string[];
}

export interface NoteReadResult {
  name: string;
  content: string;
}

export interface NoteSaveResult {
  name: string;
  saved: boolean;
}

export interface NoteDeleteResult {
  deleted: boolean;
}

export interface NoteAssetResult {
  relative: string;
  mime: string;
  data_url: string;
}

export interface NoteExportResult {
  name: string;
  format: string;
  path: string;
  bytes: number;
}
