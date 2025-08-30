import React, { useCallback, useState } from "react";
import { FileTreeNode, FileTreeItemProps } from "../types";

export const FileTreeItem = React.memo(
  ({
    item,
    onFileClick,
    onDirectoryExpand,
    selectedFile,
  }: FileTreeItemProps & {
    onDelete?: (item: FileTreeNode) => Promise<void>;
  }) => {
    const [isCollapsed, setIsCollapsed] = useState<boolean>(true);
    const [isNodeLoading, setIsNodeLoading] = useState<boolean>(false);

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

    const isSelected = selectedFile === item.fullName;

    return (
      <li style={{ listStyleType: "none" }}>
        <div
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
        >
          <span style={{ width: "1rem", textAlign: "center" }}>
            {item.type === "directory" ? (isCollapsed ? "▶" : "▼") : ""}
          </span>
          <span>{item.type === "file" ? "📄" : "📁"}</span>
          <span>{item.name}</span>
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
          {item.type === "directory" && (
            <button
              onClick={handleRefresh}
              style={{
                marginLeft: "0.5rem",
                color: "#fff",
                border: "none",
                borderRadius: "0.25rem",
                padding: "0 0.5rem",
                fontSize: "0.85em",
                cursor: "pointer",
                background: "#3b82f6",
              }}
              title={`Refresh ${item.type}`}
            >
              🔄
            </button>
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
                />
              ))}
          </ul>
        )}
      </li>
    );
  },
);
