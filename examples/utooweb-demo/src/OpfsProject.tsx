import { Project } from "@utoo/web";
import React, { useCallback, useEffect, useMemo, useState } from "react";
import { packageLock } from "./packageLock";
import MonacoEditor from "@monaco-editor/react";

import "./styles.css";

const projectName = "/utooweb-demo";

const FileTreeItem = React.memo(
  ({
    item,
    onFileClick,
    onDirectoryExpand,
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
          <span style={{ width: "1rem", textAlign: "center" }}></span>
          {item.type === "directory" ? (isCollapsed ? "▶" : "▼") : "📄"}
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
                />
              ))}
          </ul>
        )}
      </li>
    );
  },
);

const OpfsProject = () => {
  const [project, setProject] = useState<Project | null>(null);
  const [fileTree, setFileTree] = useState<FileTreeNode[]>([]);
  const [selectedFilePath, setSelectedFilePath] = useState("");
  const [selectedFileContent, setSelectedFileContent] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState("");
  const [previewUrl, setPreviewUrl] = useState<string>(""); // For page preview
  const [isBuilding, setIsBuilding] = useState(false);

  type UpdateTreeWithChildrenFn = (
    tree: FileTreeNode[],
    targetPath: string,
    newChildren: FileTreeNode[],
  ) => FileTreeNode[];

  const updateTreeWithChildren: UpdateTreeWithChildrenFn = useCallback(
    (tree, targetPath, newChildren) => {
      return tree.map((node) => {
        if (node.fullName === targetPath) {
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
    async (parentItem: DirectoryExpandParams): Promise<void> => {
      try {
        if (!project) throw new Error("Project not initialized.");

        const children: ProjectFileItem[] = await project.readdir(
          parentItem.fullName,
        );

        const newChildren: FileTreeNode[] = children
          .map((item: ProjectFileItem) => ({
            ...item,
            fullName: [parentItem.fullName, item.name]
              .filter(Boolean)
              .join("/"),
            name: item.name,
            type: item.isDirectory()
              ? ("directory" as const)
              : ("file" as const),
            children: item.isDirectory() ? [] : null,
          }))
          .sort((a, b) =>
            a.type === b.type
              ? a.name.localeCompare(b.name)
              : a.type.localeCompare(b.type),
          );

        setFileTree((prevTree) => {
          const updatedTree = updateTreeWithChildren(
            prevTree,
            parentItem.fullName,
            newChildren,
          );
          return [...updatedTree];
        });
      } catch (e: any) {
        console.error(
          `Error expanding directory at path ${parentItem.fullName}:`,
          e,
        );
        setError(`Error expanding directory: ${e.toString()}`);
      }
    },
    [project, updateTreeWithChildren],
  );

  interface FetchFileContentFn {
    (filePath: string): Promise<void>;
  }

  const fetchFileContent: FetchFileContentFn = useCallback(
    async (filePath: string): Promise<void> => {
      setSelectedFilePath(filePath);
      setSelectedFileContent("");
      try {
        if (!project) throw new Error("Project not initialized.");

        const content: string = await project.readFile(filePath, "utf8");
        setSelectedFileContent(content);

        // If index.html, generate preview URL
        if (filePath.endsWith("index.html")) {
          const blob = new Blob([content], { type: "text/html" });
          setPreviewUrl(URL.createObjectURL(blob));
        }
      } catch (e: any) {
        setError(`Error reading file: ${e.message}`);
      }
    },
    [project],
  );

  useEffect(() => {
    const tryPreviewIndex = async () => {
      if (!project) return;
      try {
        const content = await project.readFile("./index.html", "utf8");
        const blob = new Blob([content], { type: "text/html" });
        setPreviewUrl(URL.createObjectURL(blob));
      } catch {
        setPreviewUrl("");
      }
    };
    tryPreviewIndex();
  }, [project]);

  // Memoize the file tree to prevent unnecessary re-renders
  const memoizedFileTree = useMemo(() => fileTree, [fileTree]);

  useEffect(() => {
    const initialize = async () => {
      try {
        if (!navigator.storage || !navigator.storage.getDirectory) {
          throw new Error(
            "Your browser does not support the Origin Private File System.",
          );
        }

        setIsLoading(true); // Set loading before install

        const project = new Project({
          cwd: projectName,
          threadUrl: "http://localhost:8080/worker_thread.js",
        });
        setProject(project);

        console.log(
          "%cOPFS Project:%c Start to install dependencies.",
          "color: blue;",
          "color: green;",
        );

        const start = performance.now();
        await project.install(JSON.stringify(packageLock));
        await project.mkdir("src");
        await project.writeFile("src/index.tsx", "console.log('1')");
        await project.writeFile(
          "utoopack.json",
          JSON.stringify(
            {
              entry: [
                {
                  import: "./src/index.tsx",
                  name: "index",
                },
              ],
            },
            null,
            2,
          ),
        );

        console.log(
          `%cOPFS Project:%c Finished to install dependencies in ${Math.round(performance.now() - start)} ms.`,
          "color: blue;",
          "color: green;",
        );

        // new Worker(new URL("./worker", import.meta.url));

        // Read only the top-level directory without recursion
        const rootItems = await project.readdir(".");
        const initialTree = [
          {
            name: projectName,
            fullName: ".",
            type: "directory" as const,
            children: rootItems
              .map((item) => ({
                ...item,
                fullName: `./${item.name}`,
                type: item.isDirectory()
                  ? ("directory" as const)
                  : ("file" as const),
                children: item.isDirectory() ? [] : null,
              }))
              .sort((a, b) =>
                a.type === b.type
                  ? a.name.localeCompare(b.name)
                  : a.type.localeCompare(b.type),
              ),
          },
        ];
        setFileTree(initialTree);
      } catch (e) {
        const errorMessage = e instanceof Error ? e.message : String(e);
        setError(`Initialization failed: ${errorMessage}`);
      } finally {
        setIsLoading(false); // Reset loading after install
      }
    };

    initialize();
  }, []);

  // Code editor area
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
          {selectedFilePath ? selectedFilePath : "Select a file"}
        </h3>
        <div style={{ height: "calc(100% - 2rem)" }}>
          <MonacoEditor
            height="100%"
            width="100%"
            language={
              ["tsx", "ts", "jsx", "js"].some((ext) =>
                selectedFilePath.endsWith(ext),
              )
                ? "typescript"
                : selectedFilePath.endsWith(".json")
                  ? "json"
                  : selectedFilePath.endsWith(".css")
                    ? "css"
                    : selectedFilePath.endsWith(".html")
                      ? "html"
                      : "plaintext"
            }
            value={selectedFileContent}
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
    [selectedFilePath, selectedFileContent],
  );

  // Page preview area
  const previewDisplay = useMemo(
    () => (
      <div style={{ width: "100%", height: "100%", position: "relative" }}>
        <h3
          style={{
            fontSize: "1rem",
            fontWeight: "700",
            marginBottom: "0.5rem",
          }}
        >
          Preview
        </h3>
        {previewUrl ? (
          <iframe
            src={previewUrl}
            title="preview"
            style={{
              width: "100%",
              height: "calc(100% - 2rem)",
              border: "1px solid #e5e7eb",
              borderRadius: "0.5rem",
              background: "#fff",
            }}
          />
        ) : (
          <div
            style={{ color: "#9ca3af", textAlign: "center", marginTop: "2rem" }}
          >
            No index.html to preview
          </div>
        )}
      </div>
    ),
    [previewUrl],
  );

  // Build button handler
  const handleBuild = useCallback(async () => {
    if (!project) return;
    setIsBuilding(true);
    try {
      await project.build();
    } catch (e: any) {
      console.error("Build failed: ", e);
      setError(`Build failed: ${e.message}`);
    } finally {
      setIsBuilding(false);
    }
  }, [project]);

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
          display: "flex", // Add flex layout
          flexDirection: "row", // Stack items vertically
          boxSizing: "border-box", // Ensure padding is included in the width
          justifyContent: "space-between", // Space between file tree and build button
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
          <div style={{ height: "24px", width: "100%" }} />{" "}
          {/* spacer div with height */}
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
        {monacoDisplay}
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
        {previewDisplay}
      </div>
    </div>
  );
};

interface FileTreeNode {
  name: string;
  fullName: string;
  type: "directory" | "file";
  children: FileTreeNode[] | null;
}

interface FileTreeItemProps {
  item: FileTreeNode;
  onFileClick: (filePath: string) => Promise<void>;
  onDirectoryExpand?: (parentItem: FileTreeNode) => Promise<void>;
}

interface DirectoryExpandParams {
  fullName: string;
  name: string;
  type: "directory" | "file";
  children: FileTreeNode[] | null;
}

interface ProjectFileItem {
  name: string;
  isDirectory: () => boolean;
}

export default OpfsProject;

interface FileTreeNode {
  name: string;
  fullName: string;
  type: "directory" | "file";
  children: FileTreeNode[] | null;
}

interface FileTreeItemProps {
  item: FileTreeNode;
  onFileClick: (filePath: string) => Promise<void>;
  onDirectoryExpand?: (parentItem: FileTreeNode) => Promise<void>;
}

interface FileTreeItemProps {
  item: FileTreeNode;
  onFileClick: (filePath: string) => Promise<void>;
  onDirectoryExpand?: (parentItem: FileTreeNode) => Promise<void>;
}

interface DirectoryExpandParams {
  fullName: string;
  name: string;
  type: "directory" | "file";
  children: FileTreeNode[] | null;
}

interface ProjectFileItem {
  name: string;
  isDirectory: () => boolean;
}

interface FileTreeNode {
  name: string;
  fullName: string;
  type: "directory" | "file";
  children: FileTreeNode[] | null;
}

interface FileTreeItemProps {
  item: FileTreeNode;
  onFileClick: (filePath: string) => Promise<void>;
  onDirectoryExpand?: (parentItem: FileTreeNode) => Promise<void>;
  onDelete?: (item: FileTreeNode) => Promise<void>;
}

interface DirectoryExpandParams {
  fullName: string;
  name: string;
  type: "directory" | "file";
  children: FileTreeNode[] | null;
}

interface ProjectFileItem {
  name: string;
  isDirectory: () => boolean;
}
