import React, { useMemo, useState } from "react";
import { ConfigPanel } from "./components/ConfigPanel";
import { Editor } from "./components/Editor";
import { FileTreeItem } from "./components/FileTree";
import { MenuItem, MoreMenu } from "./components/MoreMenu";
import { Panel } from "./components/Panel";
import { Preview } from "./components/Preview";
import { useBuild } from "./hooks/useBuild";
import { useDev } from "./hooks/useDev";
import { useFileContent } from "./hooks/useFileContent";
import { useFileTree } from "./hooks/useFileTree";
import { useGzip } from "./hooks/useGzip";
import { useInstall } from "./hooks/useInstall";
import { useUpload } from "./hooks/useUpload";
import { useUtooProject } from "./hooks/useUtooProject";
import {
  deleteProjectFiles,
  getConfig,
  UtooConfig,
} from "./services/utooService";
import "./styles.css";

const Project = () => {
  const { project, isLoading, error: projectError } = useUtooProject();
  const {
    fileTree,
    handleDirectoryExpand,
    refreshFileTree,
    createFile,
    createFolder,
    deleteItem,
  } = useFileTree(project);
  const [isConfigOpen, setIsConfigOpen] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [config, setConfig] = useState<UtooConfig>(getConfig);
  const {
    selectedFilePath,
    selectedFileContent,
    setSelectedFileContent,
    previewUrl,
    fetchFileContent,
    saveFile,
    isSaving,
    hasUnsavedChanges,
    justSaved,
    error: fileContentError,
  } = useFileContent(project);

  const previewRef = React.useRef<{ reload: () => void }>(null);

  // HMR iframe connection handler
  const handleConnectHmrIframe = React.useCallback(
    (iframe: HTMLIFrameElement) => {
      if (!project) return null;
      return project.connectHmrIframe(iframe);
    },
    [project],
  );

  const {
    isInstalling,
    isUpdating,
    handleInstall,
    handleUpdate,
    error: installError,
    hasLock,
  } = useInstall(project, config);

  const handleDeleteProject = async () => {
    const confirmed = window.confirm(
      "Are you sure you want to delete all project files? This action cannot be undone.",
    );
    if (!confirmed) return;

    setIsDeleting(true);
    try {
      await deleteProjectFiles(project!);
      window.location.reload();
    } catch (e) {
      console.error("Failed to delete project:", e);
      setIsDeleting(false);
    }
  };

  const {
    isDevMode,
    isBuilding: isDevBuilding,
    error: devError,
    startDev,
    stopDev,
  } = useDev(project, fileTree, handleDirectoryExpand, {
    // HMR will handle updates automatically, no need to manually reload
    onBuildComplete: undefined,
  });

  const {
    isBuilding: isBuildingOnce,
    handleBuild,
    error: buildError,
  } = useBuild(project, fileTree, handleDirectoryExpand, () => {
    if (previewRef.current) {
      previewRef.current.reload();
    }
  });

  const {
    isGzipping,
    handleGzip,
    error: gzipError,
    gzipSuccess,
  } = useGzip(project);

  const {
    isUploading,
    handleUpload,
    error: uploadError,
  } = useUpload(project, refreshFileTree);

  const isBuilding = isDevBuilding || isBuildingOnce;

  const error =
    projectError ||
    fileContentError ||
    installError ||
    devError ||
    buildError ||
    gzipError ||
    uploadError;

  const memoizedFileTree = useMemo(() => fileTree, [fileTree]);

  const isWorking = isInstalling || isUpdating;
  const installButton = (
    <button
      data-testid="install-dependencies-button"
      onClick={hasLock ? handleUpdate : handleInstall}
      disabled={isWorking || !project}
      style={{
        padding: "0.25rem 0.75rem",
        borderRadius: "0.375rem",
        border: "none",
        fontSize: "0.875rem",
        background: isWorking ? "#d1d5db" : "#10b981",
        color: "#fff",
        fontWeight: 500,
        cursor: isWorking ? "not-allowed" : "pointer",
        transition: "background 0.2s",
      }}
    >
      {isUpdating
        ? "Updating..."
        : isInstalling
          ? "Installing..."
          : hasLock
            ? "Update"
            : "Install"}
    </button>
  );

  const buildButton = (
    <button
      data-testid="build-project-button"
      onClick={handleBuild}
      disabled={isBuilding || !project}
      style={{
        padding: "0.25rem 0.75rem",
        borderRadius: "0.375rem",
        border: "none",
        fontSize: "0.875rem",
        background: isBuildingOnce ? "#d1d5db" : "#6366f1",
        color: "#fff",
        fontWeight: 500,
        cursor: isBuilding ? "not-allowed" : "pointer",
        transition: "background 0.2s",
        marginLeft: "0.5rem",
      }}
    >
      {isBuildingOnce ? "Building..." : "Build"}
    </button>
  );

  const devButton = (
    <button
      data-testid="dev-project-button"
      onClick={isDevMode ? stopDev : startDev}
      disabled={isBuilding || !project}
      style={{
        padding: "0.25rem 0.75rem",
        borderRadius: "0.375rem",
        border: "none",
        fontSize: "0.875rem",
        background: isDevBuilding
          ? "#d1d5db"
          : isDevMode
            ? "#10b981"
            : "#2563eb",
        color: "#fff",
        fontWeight: 500,
        cursor: isBuilding ? "not-allowed" : "pointer",
        transition: "background 0.2s",
        marginLeft: "0.5rem",
      }}
    >
      {isDevBuilding ? "Building..." : isDevMode ? "Watching" : "Dev"}
    </button>
  );

  const moreMenuItems: MenuItem[] = [
    {
      label: isUploading ? "Uploading..." : "Upload Demo",
      onClick: handleUpload,
      disabled: isUploading || !project || isBuilding,
      testId: "upload-demo-menu-item",
    },
    {
      label: isGzipping
        ? "Downloading..."
        : gzipSuccess
          ? "Downloaded"
          : "Download",
      onClick: handleGzip,
      disabled: isGzipping || !project || isBuilding,
      testId: "download-project-menu-item",
    },
    {
      label: "Settings",
      onClick: () => setIsConfigOpen(true),
      disabled: !project,
      testId: "settings-menu-item",
    },
    {
      label: isDeleting ? "Resetting..." : "Reset Project",
      onClick: handleDeleteProject,
      disabled: isDeleting || !project || isInstalling || isBuilding,
      danger: true,
      testId: "reset-project-menu-item",
    },
  ];

  const saveButton = (
    <button
      data-testid="save-file-button"
      onClick={saveFile}
      disabled={isSaving || !hasUnsavedChanges || !project}
      style={{
        padding: "0.25rem 0.75rem",
        borderRadius: "0.375rem",
        border: "none",
        fontSize: "0.875rem",
        background: isSaving
          ? "#fbbf24"
          : justSaved
            ? "#22c55e"
            : hasUnsavedChanges
              ? "#10b981"
              : "#d1d5db",
        color: "#fff",
        fontWeight: 500,
        cursor:
          isSaving || !hasUnsavedChanges || !project
            ? "not-allowed"
            : "pointer",
        transition: "background 0.3s",
      }}
    >
      {isSaving
        ? "Saving..."
        : justSaved
          ? "✓ Saved"
          : hasUnsavedChanges
            ? "Save"
            : "—"}
    </button>
  );

  return (
    <div
      data-testid="utooweb-demo"
      style={{
        height: "100vh",
        display: "flex",
        flexDirection: "row",
        backgroundColor: "#f3f4f6",
        fontFamily: "sans-serif",
      }}
    >
      <ConfigPanel
        isOpen={isConfigOpen}
        onClose={() => setIsConfigOpen(false)}
        onConfigChange={setConfig}
      />

      <Panel
        testId="project-panel"
        title="Project"
        actions={
          <>
            {installButton}
            {buildButton}
            {devButton}
            <MoreMenu
              testId="project-more-menu"
              items={moreMenuItems}
              disabled={!project}
            />
          </>
        }
        style={{
          width: "25%",
          minWidth: "300px",
          borderRight: "1px solid #e5e7eb",
        }}
        contentStyle={{ padding: "0.5rem 1rem" }}
      >
        {isLoading && (
          <p
            data-testid="project-initializing"
            style={{ textAlign: "center", color: "#22c55e", fontWeight: 500 }}
          >
            Initializing project...
          </p>
        )}
        {error && (
          <p
            data-testid="project-error"
            style={{ textAlign: "center", color: "#ef4444" }}
          >
            {error}
          </p>
        )}
        {!isLoading && !error && (
          <ul
            data-testid="project-file-tree"
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
                selectedFile={selectedFilePath}
                onCreateFile={createFile}
                onCreateFolder={createFolder}
                onDelete={deleteItem}
              />
            ))}
          </ul>
        )}
      </Panel>

      <Panel
        testId="editor-panel"
        title={`Editor${hasUnsavedChanges ? " *" : ""}`}
        actions={saveButton}
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
          onSave={saveFile}
          autoSaveDelay={1000}
        />
      </Panel>

      <Panel
        testId="preview-panel"
        title="Preview"
        style={{ width: "35%", minWidth: "320px" }}
        contentStyle={{ padding: "1rem" }}
      >
        <Preview
          ref={previewRef}
          url={previewUrl}
          onIframeReady={handleConnectHmrIframe}
        />
      </Panel>
    </div>
  );
};

export default Project;
