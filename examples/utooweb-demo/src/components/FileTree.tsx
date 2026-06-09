import React, { useCallback, useState } from "react";
import { FileTreeItemProps, FileTreeNode } from "../types";

interface ExtendedFileTreeItemProps extends FileTreeItemProps {
  onCreateFile?: (parentPath: string, fileName: string) => Promise<boolean>;
  onCreateFolder?: (parentPath: string, folderName: string) => Promise<boolean>;
  onDelete?: (item: FileTreeNode) => Promise<boolean>;
}

const getFileTreeTestIdPart = (fullName: string): string => {
  const path = fullName === "." ? "root" : fullName.replace(/^\.\//, "");
  return (
    path
      .replace(/[^a-zA-Z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .toLowerCase() || "root"
  );
};

export const FileTreeItem = React.memo(
  ({
    item,
    onFileClick,
    onDirectoryExpand,
    selectedFile,
    onCreateFile,
    onCreateFolder,
    onDelete,
  }: ExtendedFileTreeItemProps) => {
    const [isCollapsed, setIsCollapsed] = useState<boolean>(true);
    const [isNodeLoading, setIsNodeLoading] = useState<boolean>(false);
    const [showActions, setShowActions] = useState(false);

    const toggleCollapse = useCallback(async () => {
      if (item.type === "directory") {
        if (isCollapsed) {
          if (item.children && item.children.length === 0) {
            setIsNodeLoading(true);
            await onDirectoryExpand?.(item);
            setIsNodeLoading(false);
          }
        }
        setIsCollapsed(!isCollapsed);
      } else {
        onFileClick(item.fullName);
      }
    }, [item, isCollapsed, onFileClick, onDirectoryExpand]);

    const handleRefresh = useCallback(
      async (e: React.MouseEvent) => {
        e.stopPropagation();
        if (item.type === "directory") {
          await onDirectoryExpand?.(item);
        }
      },
      [onDirectoryExpand, item],
    );

    const handleNewFile = useCallback(
      async (e: React.MouseEvent) => {
        e.stopPropagation();
        const fileName = prompt("Enter file name:");
        if (fileName && onCreateFile) {
          await onCreateFile(item.fullName, fileName);
        }
      },
      [item.fullName, onCreateFile],
    );

    const handleNewFolder = useCallback(
      async (e: React.MouseEvent) => {
        e.stopPropagation();
        const folderName = prompt("Enter folder name:");
        if (folderName && onCreateFolder) {
          await onCreateFolder(item.fullName, folderName);
        }
      },
      [item.fullName, onCreateFolder],
    );

    const handleDelete = useCallback(
      async (e: React.MouseEvent) => {
        e.stopPropagation();
        const confirmed = confirm(
          `Are you sure you want to delete "${item.name}"?`,
        );
        if (confirmed && onDelete) {
          await onDelete(item);
        }
      },
      [item, onDelete],
    );

    const isSelected = selectedFile === item.fullName;
    const isRootDir = item.fullName === ".";
    const testIdPart = getFileTreeTestIdPart(item.fullName);
    const nodeTestId = `file-tree-${item.type}-${testIdPart}`;

    const actionButtonStyle: React.CSSProperties = {
      color: "#fff",
      border: "none",
      borderRadius: "0.25rem",
      padding: "0 0.35rem",
      fontSize: "0.75em",
      cursor: "pointer",
      marginLeft: "0.25rem",
    };

    return (
      <li style={{ listStyleType: "none" }}>
        <div
          data-testid={nodeTestId}
          data-file-path={item.fullName}
          data-file-type={item.type}
          aria-expanded={item.type === "directory" ? !isCollapsed : undefined}
          style={{
            display: "flex",
            alignItems: "center",
            gap: "0.5rem",
            padding: "0.25rem 0.5rem",
            borderRadius: "0.5rem",
            cursor: "pointer",
            transition: "background-color 0.2s ease-in-out",
            backgroundColor: isSelected ? "rgba(0, 0, 0, 0.1)" : "transparent",
          }}
          onClick={toggleCollapse}
          onMouseEnter={() => setShowActions(true)}
          onMouseLeave={() => setShowActions(false)}
        >
          <span style={{ width: "1rem", textAlign: "center" }}>
            {item.type === "directory" ? (isCollapsed ? "▶" : "▼") : ""}
          </span>
          <span>{item.type === "file" ? "📄" : "📁"}</span>
          <span style={{ flex: 1 }}>{item.name}</span>
          {isNodeLoading && (
            <span
              style={{
                marginLeft: "0.5rem",
                color: "#22c55e",
                fontSize: "0.85em",
              }}
            >
              Loading...
            </span>
          )}
          {showActions && (
            <span
              style={{ display: "flex", alignItems: "center" }}
              onClick={(e) => e.stopPropagation()}
            >
              {item.type === "directory" && (
                <>
                  <button
                    data-testid={`file-tree-refresh-${testIdPart}`}
                    onClick={handleRefresh}
                    style={{ ...actionButtonStyle, background: "#3b82f6" }}
                    title="Refresh"
                  >
                    🔄
                  </button>
                  <button
                    data-testid={`file-tree-new-file-${testIdPart}`}
                    onClick={handleNewFile}
                    style={{ ...actionButtonStyle, background: "#10b981" }}
                    title="New File"
                  >
                    +📄
                  </button>
                  <button
                    data-testid={`file-tree-new-folder-${testIdPart}`}
                    onClick={handleNewFolder}
                    style={{ ...actionButtonStyle, background: "#8b5cf6" }}
                    title="New Folder"
                  >
                    +📁
                  </button>
                </>
              )}
              {!isRootDir && (
                <button
                  data-testid={`file-tree-delete-${testIdPart}`}
                  onClick={handleDelete}
                  style={{ ...actionButtonStyle, background: "#ef4444" }}
                  title="Delete"
                >
                  🗑
                </button>
              )}
            </span>
          )}
        </div>
        {item.type === "directory" && !isCollapsed && (
          <ul
            style={{
              paddingLeft: "1rem",
              borderLeft: "1px solid #d1d5db",
              marginLeft: "0.5rem",
              marginTop: "0.25rem",
            }}
          >
            {item.children &&
              item.children.map((child: FileTreeNode) => (
                <FileTreeItem
                  key={child.fullName}
                  item={child}
                  onFileClick={onFileClick}
                  onDirectoryExpand={onDirectoryExpand}
                  selectedFile={selectedFile}
                  onCreateFile={onCreateFile}
                  onCreateFolder={onCreateFolder}
                  onDelete={onDelete}
                />
              ))}
          </ul>
        )}
      </li>
    );
  },
);
