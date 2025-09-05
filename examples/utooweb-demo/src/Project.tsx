import React, { useMemo } from "react";
import { Editor } from "./components/Editor";
import { FileTreeItem } from "./components/FileTree";
import { Panel } from "./components/Panel";
import { Preview } from "./components/Preview";
import { useBuild } from "./hooks/useBuild";
import { useFileContent } from "./hooks/useFileContent";
import { useFileTree } from "./hooks/useFileTree";
import { useUtooProject } from "./hooks/useUtooProject";
import "./styles.css";

const Project = () => {
  const { project, isLoading, error: projectError } = useUtooProject();
  const { fileTree, handleDirectoryExpand } = useFileTree(project);
  const {
    selectedFilePath,
    selectedFileContent,
    setSelectedFileContent,
    previewUrl,
    fetchFileContent,
    error: fileContentError,
  } = useFileContent(project);

  const previewRef = React.useRef<{ reload: () => void }>(null);

  const {
    isBuilding,
    handleBuild,
    error: buildError,
  } = useBuild(project, fileTree, handleDirectoryExpand, () => {
    if (previewRef.current) {
      previewRef.current.reload();
    }
  });

  const error = projectError || fileContentError || buildError;

  const memoizedFileTree = useMemo(() => fileTree, [fileTree]);

  const buildButton = (
    <button
      onClick={handleBuild}
      disabled={isBuilding || !project}
      style={{
        padding: "0.25rem 0.75rem",
        borderRadius: "0.375rem",
        border: "none",
        fontSize: "0.875rem",
        background: isBuilding ? "#d1d5db" : "#2563eb",
        color: "#fff",
        fontWeight: 500,
        cursor: isBuilding ? "not-allowed" : "pointer",
        transition: "background 0.2s",
      }}
    >
      {isBuilding ? "Building..." : "Build"}
    </button>
  );

  return (
    <div
      style={{
        height: "100vh",
        display: "flex",
        flexDirection: "row",
        backgroundColor: "#f3f4f6",
        fontFamily: "sans-serif",
      }}
    >
      <Panel
        title="Project"
        actions={buildButton}
        style={{
          width: "25%",
          minWidth: "300px",
          borderRight: "1px solid #e5e7eb",
        }}
        contentStyle={{ padding: "0.5rem 1rem" }}
      >
        {isLoading && (
          <p style={{ textAlign: "center", color: "#22c55e", fontWeight: 500 }}>
            Installing dependencies...
          </p>
        )}
        {error && (
          <p style={{ textAlign: "center", color: "#ef4444" }}>{error}</p>
        )}
        {!isLoading && !error && (
          <ul
            style={{
              display: "flex",
              flexDirection: "column",
              gap: "0.25rem",
              padding: 0,
              alignItems: "flex-start",
            }}
          >
            {memoizedFileTree.map((item, index) => (
              <FileTreeItem
                key={index}
                item={item}
                onFileClick={fetchFileContent}
                onDirectoryExpand={
                  item.type === "directory" ? handleDirectoryExpand : undefined
                }
                selectedFile={selectedFilePath} // Pass the selectedFilePath state here
              />
            ))}
          </ul>
        )}
      </Panel>

      <Panel
        title="Editor"
        style={{
          width: "40%",
          minWidth: "320px",
          borderRight: "1px solid #e5e7eb",
        }}
        contentStyle={{ paddingTop: "0.5rem" }}
      >
        <Editor
          filePath={selectedFilePath}
          content={selectedFileContent}
          onContentChange={setSelectedFileContent}
        />
      </Panel>

      <Panel
        title="Preview"
        style={{ width: "35%", minWidth: "320px" }}
        contentStyle={{ padding: "1rem" }}
      >
        <Preview ref={previewRef} url={previewUrl} />
      </Panel>
    </div>
  );
};

export default Project;
