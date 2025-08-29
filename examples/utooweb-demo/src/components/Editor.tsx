import React from "react";
import MonacoEditor from "@monaco-editor/react";

interface EditorProps {
    filePath: string;
    content: string;
    onContentChange: (newContent: string) => void;
}

export const Editor = ({ filePath, content, onContentChange }: EditorProps) => {
    return (
        <MonacoEditor
            height="100%"
            width="100%"
            path={filePath}
            language={
                ["tsx", "ts", "jsx", "js"].some((ext) =>
                    filePath.endsWith(ext),
                )
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
            onChange={(value) => onContentChange(value || "")}
            options={{
                readOnly: false,
                fontSize: 14,
                minimap: { enabled: false },
                scrollBeyondLastLine: false,
            }}
        />
    );
};
