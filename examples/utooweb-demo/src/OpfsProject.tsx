import { Project } from "@utoo/web";
import React, { useEffect, useState, useCallback, useMemo } from "react";
import { packageLock } from "./packageLock";

import "./styles.css";

// Extract FileTreeItem as a separate component to prevent recreation
const FileTreeItem = React.memo(
  ({
    item,
    onFileClick,
    onDirectoryExpand,
  }: {
    item: any;
    onFileClick: (filePath: string) => Promise<void>;
    onDirectoryExpand: (parentItem: any) => Promise<void>;
  }) => {
    const [isCollapsed, setIsCollapsed] = useState(true);

    const toggleCollapse = useCallback(async () => {
      if (item.type === "directory") {
        if (isCollapsed) {
          if (item.children && item.children.length === 0) {
            await onDirectoryExpand(item);
          }
        }
        setIsCollapsed(!isCollapsed);
      } else {
        onFileClick(item.fullName);
      }
    }, [item, isCollapsed, onFileClick, onDirectoryExpand]);

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
          }}
          onClick={toggleCollapse}
        >
          <span style={{ width: "1rem", textAlign: "center" }}>
            {item.type === "directory" ? (isCollapsed ? "▶" : "▼") : "📄"}
          </span>
          <span>{item.name}</span>
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
            {item.children.map((child) => (
              <FileTreeItem
                key={child.fullName}
                item={child}
                onFileClick={onFileClick}
                onDirectoryExpand={onDirectoryExpand}
              />
            ))}
          </ul>
        )}
      </li>
    );
  },
);

const OpfsProject = () => {
  const [project, setProject] = useState(null);
  const [fileTree, setFileTree] = useState([]);
  const [selectedFilePath, setSelectedFilePath] = useState("");
  const [selectedFileContent, setSelectedFileContent] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState("");

  const updateTreeWithChildren = useCallback(
    (tree, targetPath, newChildren) => {
      return tree.map((node) => {
        if (node.name === targetPath) {
          // Found the node, return a new object with the updated children
          return { ...node, children: newChildren };
        }
        if (
          node.type === "directory" &&
          node.children &&
          node.children.length > 0
        ) {
          return {
            ...node,
            children: updateTreeWithChildren(
              node.children,
              targetPath,
              newChildren,
            ),
          };
        }
        return node;
      });
    },
    [],
  );

  const handleDirectoryExpand = useCallback(
    async (parentItem) => {
      try {
        if (!project) throw new Error("Project not initialized.");

        const children = await project.readDir(parentItem.fullName);

        const newChildren = children.map((item) => ({
          ...item,
          fullName: [parentItem.fullName, item.name].filter(Boolean).join("/"),
          children: item.type === "directory" ? [] : null,
        }));

        setFileTree((prevTree) =>
          updateTreeWithChildren(prevTree, parentItem.name, newChildren),
        );
      } catch (e) {
        console.error(
          `Error expanding directory at path ${parentItem.fullName}:`,
          e,
        );
        setError(`Error expanding directory: ${e.message}`);
      }
    },
    [project, updateTreeWithChildren],
  );

  const fetchFileContent = useCallback(
    async (filePath) => {
      setSelectedFilePath(filePath);
      setSelectedFileContent("");
      try {
        if (!project) throw new Error("Project not initialized.");

        const content = await project.readFile(filePath);
        setSelectedFileContent(content);
      } catch (e) {
        setError(`Error reading file: ${e.message}`);
      }
    },
    [project],
  );

  // Memoize the file tree to prevent unnecessary re-renders
  const memoizedFileTree = useMemo(() => fileTree, [fileTree]);

  // Memoize the file content display to prevent re-rendering when other states change
  const fileContentDisplay = useMemo(() => {
    if (!selectedFilePath) {
      return (
        <p style={{ textAlign: "center", color: "#9ca3af", margin: "auto" }}>
          Click a file on the left to view its content.
        </p>
      );
    }

    return (
      <>
        <h3
          style={{
            fontSize: "1.25rem",
            fontWeight: "700",
            marginBottom: "0.5rem",
          }}
        >
          File Path: {selectedFilePath}
        </h3>
        <textarea
          id="code-mirror-editor"
          value={selectedFileContent}
          style={{
            flex: "1",
            minHeight: "300px",
            backgroundColor: "#f3f4f6",
            padding: "1rem",
            borderRadius: "0.5rem",
            fontSize: "0.875rem",
            width: "100%",
            boxSizing: "border-box",
            resize: "none",
          }}
          readOnly
        />
      </>
    );
  }, [selectedFilePath, selectedFileContent]);

  useEffect(() => {
    const initialize = async () => {
      try {
        if (!navigator.storage || !navigator.storage.getDirectory) {
          throw new Error(
            "Your browser does not support the Origin Private File System.",
          );
        }

        const project = new Project("/utooweb-demo");
        setProject(project);

        console.log(
          "%cOPFS Project:%c Start to install dependencies.",
          "color: blue;",
          "color: green;",
        );

        const start = performance.now();
        await project.install(JSON.stringify(packageLock));

        console.log(
          `%cOPFS Project:%c Finished to install dependencies in ${Math.round(performance.now() - start)} ms.`,
          "color: blue;",
          "color: green;",
        );

        new Worker(new URL("./worker", import.meta.url));

        // Read only the top-level directory without recursion
        const rootItems = await project.readDir(".");
        const initialTree = rootItems.map((item) => ({
          ...item,
          fullName: `./${item.name}`,
          children: item.type === "directory" ? [] : null,
        }));
        setFileTree(initialTree);
      } catch (e) {
        setError(`Initialization failed: ${e.message}`);
      } finally {
        setIsLoading(false);
      }
    };

    initialize();
  }, []);

  return (
    <div
      style={{
        height: "100vh",
        padding: "1rem",
        display: "flex",
        flexDirection: "row",
        gap: "1rem",
        backgroundColor: "#f3f4f6",
        fontFamily: "sans-serif",
      }}
    >
      <div
        style={{
          width: "40%",
          padding: "1.5rem",
          backgroundColor: "#ffffff",
          borderRadius: "0.75rem",
          boxShadow:
            "0 4px 6px -1px rgba(0, 0, 0, 0.1), 0 2px 4px -1px rgba(0, 0, 0, 0.06)",
          overflowY: "auto",
        }}
      >
        <h2
          style={{
            fontSize: "1.5rem",
            fontWeight: "700",
            marginBottom: "1rem",
            paddingBottom: "0.5rem",
            borderBottom: "2px solid #e5e7eb",
            color: "#374151",
          }}
        >
          OPFS Project Explorer
        </h2>
        {isLoading && (
          <p style={{ textAlign: "center", color: "#6b7280" }}>
            Loading file system...
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
            }}
          >
            {memoizedFileTree.map((item, index) => (
              <FileTreeItem
                key={index}
                item={item}
                onFileClick={fetchFileContent}
                onDirectoryExpand={handleDirectoryExpand}
              />
            ))}
          </ul>
        )}
      </div>

      <div
        style={{
          width: "60%",
          padding: "1.5rem",
          backgroundColor: "#ffffff",
          borderRadius: "0.75rem",
          boxShadow:
            "0 4px 6px -1px rgba(0, 0, 0, 0.1), 0 2px 4px -1px rgba(0, 0, 0, 0.06)",
          overflowY: "auto",
          display: "flex",
          flexDirection: "column",
          justifyContent: "center",
          alignItems: "center",
        }}
      >
        {fileContentDisplay}
      </div>
    </div>
  );
};

export default OpfsProject;
