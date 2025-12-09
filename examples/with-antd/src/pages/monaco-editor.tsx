import Editor, { DiffEditor } from "@monaco-editor/react";
import React from "react";

export function MonacoEditor() {
  return (
    <div>
      <h2>Monaco Editor</h2>
      <Editor
        height="50vh"
        language="typescript"
        theme="vs-dark"
        value={`function hello() {
  console.log("Hello, world!");
}

hello();
`}
      />
      <DiffEditor
        height="50vh"
        original={`// .umirc.ts
export default {
}`}
        modified={`// .umirc.ts
export default {
  clientLoader: {} 
}`}
        language="typescript"
      />
    </div>
  );
}
