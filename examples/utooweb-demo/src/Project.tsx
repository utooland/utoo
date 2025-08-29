import React, { useMemo } from "react";
import { useUtooProject } from "./hooks/useUtooProject";
import { useFileTree } from "./hooks/useFileTree";
import { useFileContent } from "./hooks/useFileContent";
import { useBuild } from "./hooks/useBuild";
import { FileTreeItem } from "./components/FileTree";
import { Editor } from "./components/Editor";
import { Preview } from "./components/Preview";
import "./styles.css";

const Project = () => {
  const { project, isLoading, error: projectError } = useUtooProject();
  const { fileTree, handleDirectoryExpand } = useFileTree(project);
  const {
    selectedFilePath,
    selectedFileContent,
    previewUrl,
    fetchFileContent,
    error: fileContentError,
  } = useFileContent(project);
  const { isBuilding, handleBuild, error: buildError } = useBuild(project, fileTree, handleDirectoryExpand);

  const error = projectError || fileContentError || buildError;

  const memoizedFileTree = useMemo(() => fileTree, [fileTree]);

  return (
    <div
      style={{
        height: "100vh",
        padding: "0",
        display: "flex",
        flexDirection: "row",
        gap: "0",
        backgroundColor: "#f3f4f6",
        fontFamily: "sans-serif",
      }}
    >
      {/* Left file tree */}
      <div
        style={{
          width: "400px",
          padding: "1rem",
          backgroundColor: "#ffffff",
          borderRight: "1px solid #e5e7eb",
          overflowY: "auto",
          display: "flex",
          flexDirection: "row",
          boxSizing: "border-box",
          justifyContent: "space-between",
        }}
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
              />
            ))}
          </ul>
        )}
        <div
          style={{
            marginRight: "4px",
            marginTop: "-8px",
          }}
        >
          <div style={{ height: "24px", width: "100%" }} />
          <button
            onClick={handleBuild}
            disabled={isBuilding || !project}
            style={{
              padding: "0.3rem 1rem",
              borderRadius: "0.5rem",
              border: "none",
              background: isBuilding ? "#d1d5db" : "#2563eb",
              color: "#fff",
              fontWeight: 600,
              cursor: isBuilding ? "not-allowed" : "pointer",
              transition: "background 0.2s",
            }}
          >
            {isBuilding ? "Building..." : "Build"}
          </button>
        </div>
      </div>

      {/* Middle code editor */}
      <div
        style={{
          width: "35%",
          minWidth: "320px",
          padding: "1rem",
          backgroundColor: "#ffffff",
          borderRight: "1px solid #e5e7eb",
          overflowY: "auto",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <Editor filePath={selectedFilePath} content={selectedFileContent} />
      </div>

      {/* Right page preview */}
      <div
        style={{
          width: "35%",
          minWidth: "320px",
          padding: "1rem",
          backgroundColor: "#ffffff",
          overflowY: "auto",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <Preview url={previewUrl} />
      </div>
    </div>
  );
};

export default Project;
