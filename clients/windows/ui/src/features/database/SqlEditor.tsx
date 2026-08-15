import { useEffect, useRef } from "react";
import * as monaco from "monaco-editor";

const SQL_KEYWORDS = [
  "SELECT", "FROM", "WHERE", "INSERT", "INTO", "VALUES", "UPDATE", "SET",
  "DELETE", "CREATE", "TABLE", "VIEW", "INDEX", "DROP", "ALTER", "JOIN",
  "LEFT", "RIGHT", "INNER", "OUTER", "ON", "AS", "AND", "OR", "NOT",
  "NULL", "ORDER", "BY", "GROUP", "HAVING", "LIMIT", "OFFSET", "UNION",
  "DISTINCT", "COUNT", "SUM", "AVG", "MIN", "MAX", "PRIMARY", "KEY",
  "FOREIGN", "REFERENCES", "DEFAULT", "UNIQUE", "DESC", "ASC", "EXISTS",
];

interface SqlCompletionSource {
  name: string;
  kind: string;
}

let completionRegistered = false;

function ensureSqlCompletion(getSources: () => SqlCompletionSource[]) {
  if (completionRegistered) {
    return;
  }
  completionRegistered = true;
  monaco.languages.registerCompletionItemProvider("sql", {
    provideCompletionItems(model, position) {
      const word = model.getWordUntilPosition(position);
      const range: monaco.IRange = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endColumn: word.endColumn,
      };
      const keywords = SQL_KEYWORDS.map((keyword) => ({
        label: keyword,
        kind: monaco.languages.CompletionItemKind.Keyword,
        insertText: keyword,
        range,
      }));
      const objects = getSources().map((obj) => ({
        label: obj.name,
        kind: monaco.languages.CompletionItemKind.Field,
        insertText: obj.name,
        detail: obj.kind,
        range,
      }));
      return { suggestions: [...keywords, ...objects] };
    },
  });
}

interface SqlEditorProps {
  value: string;
  onChange: (value: string) => void;
  onRun: () => void;
  objectNames: string[];
}

export function SqlEditor({ value, onChange, onRun, objectNames }: SqlEditorProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const onRunRef = useRef(onRun);
  const objectNamesRef = useRef(objectNames);

  useEffect(() => {
    onRunRef.current = onRun;
  }, [onRun]);
  useEffect(() => {
    objectNamesRef.current = objectNames;
  }, [objectNames]);

  useEffect(() => {
    if (!containerRef.current) {
      return;
    }
    ensureSqlCompletion(() => objectNamesRef.current.map((name) => ({ name, kind: "table" })));
    const editor = monaco.editor.create(containerRef.current, {
      value,
      language: "sql",
      theme: "vs",
      fontSize: 12,
      minimap: { enabled: false },
      automaticLayout: true,
      scrollBeyondLastLine: false,
    });
    editorRef.current = editor;
    editor.onDidChangeModelContent(() => {
      onChange(editor.getValue());
    });
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => onRunRef.current());
    return () => {
      editor.dispose();
      editorRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const editor = editorRef.current;
    if (editor && editor.getValue() !== value) {
      editor.setValue(value);
    }
  }, [value]);

  return (
    <div
      ref={containerRef}
      style={{
        width: "100%",
        height: 180,
        border: "1px solid #d1d5db",
        borderRadius: 4,
        overflow: "hidden",
      }}
    />
  );
}
