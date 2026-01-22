import MonacoEditor from "@monaco-editor/react";
import React, { useCallback, useEffect, useRef } from "react";

interface EditorProps {
  filePath: string;
  content: string;
  onContentChange: (newContent: string) => void;
  onSave?: () => void;
  autoSaveDelay?: number; // milliseconds, default 1000ms
}

export const Editor = ({
  filePath,
  content,
  onContentChange,
  onSave,
  autoSaveDelay = 10,
}: EditorProps) => {
  const modelUri = filePath
    ? `file:///${filePath.replace(/^\.\//, "")}`
    : undefined;

  const autoSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isFirstRenderRef = useRef(true);

  // Auto-save with debounce
  const scheduleAutoSave = useCallback(() => {
    if (autoSaveTimerRef.current) {
      clearTimeout(autoSaveTimerRef.current);
    }

    autoSaveTimerRef.current = setTimeout(() => {
      onSave?.();
    }, autoSaveDelay);
  }, [onSave, autoSaveDelay]);

  // Cleanup timer on unmount
  useEffect(() => {
    return () => {
      if (autoSaveTimerRef.current) {
        clearTimeout(autoSaveTimerRef.current);
      }
    };
  }, []);

  // Reset first render flag when file changes
  useEffect(() => {
    isFirstRenderRef.current = true;
  }, [filePath]);

  const handleChange = useCallback(
    (value: string | undefined) => {
      onContentChange(value || "");

      // Skip auto-save on first render (when file is loaded)
      if (isFirstRenderRef.current) {
        isFirstRenderRef.current = false;
        return;
      }

      // Schedule auto-save
      if (onSave) {
        scheduleAutoSave();
      }
    },
    [onContentChange, onSave, scheduleAutoSave],
  );

  if (!filePath) {
    return null;
  }

  return (
    <MonacoEditor
      height="100%"
      width="100%"
      path={modelUri}
      language={
        ["tsx", "ts", "jsx", "js"].some((ext) => filePath.endsWith(ext))
          ? "typescript"
          : filePath.endsWith(".json")
            ? "json"
            : filePath.endsWith(".css")
              ? "css"
              : filePath.endsWith(".html")
                ? "html"
                : "plaintext"
      }
      value={content}
      onChange={handleChange}
      options={{
        readOnly: false,
        fontSize: 14,
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
      }}
    />
  );
};
