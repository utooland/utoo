import React, { useMemo } from "react";
import MonacoEditor from "@monaco-editor/react";

interface EditorProps {
    filePath: string;
    content: string;
}

export const Editor = ({ filePath, content }: EditorProps) => {
    const monacoDisplay = useMemo(
        () => (
            <div style={{ height: "100%", width: "100%" }}>
                <h3
                    style={{
                        fontSize: "1rem",
                        fontWeight: "700",
                        marginBottom: "0.5rem",
                    }}
                >
                    {filePath ? filePath : "Select a file"}
                </h3>
                <div style={{ height: "calc(100% - 2rem)" }}>
                    <MonacoEditor
                        height="100%"
                        width="100%"
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
                        options={{
                            readOnly: true,
                            fontSize: 14,
                            minimap: { enabled: false },
                            scrollBeyondLastLine: false,
                        }}
                    />
                </div>
            </div>
        ),
        [filePath, content],
    );

    return monacoDisplay;
};
