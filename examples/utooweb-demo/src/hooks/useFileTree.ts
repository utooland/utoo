import { Project as UtooProject } from "@utoo/web";
import { useCallback, useEffect, useState } from "react";
import { DirectoryExpandParams, FileTreeNode, ProjectFileItem } from "../types";

const projectName = "/utooweb-demo";

export const useFileTree = (project: UtooProject | null) => {
  const [fileTree, setFileTree] = useState<FileTreeNode[]>([]);
  const [error, setError] = useState<string>("");

  const buildInitialFileTree = useCallback(async (proj: UtooProject) => {
    const rootItems = await proj.readdir(".");
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
  }, []);

  useEffect(() => {
    if (project) {
      buildInitialFileTree(project);
    }
  }, [project, buildInitialFileTree]);

  const updateTreeWithChildren = useCallback(
    (
      tree: FileTreeNode[],
      targetPath: string,
      newChildren: FileTreeNode[],
    ): FileTreeNode[] => {
      return tree.map((node) => {
        if (node.fullName === targetPath) {
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
      }
    },
    [project, updateTreeWithChildren],
  );

  const refreshFileTree = useCallback(async () => {
    if (project) {
      await buildInitialFileTree(project);
    }
  }, [project, buildInitialFileTree]);

  const createFile = useCallback(
    async (parentPath: string, fileName: string): Promise<boolean> => {
      if (!project) {
        setError("Project not initialized");
        return false;
      }

      try {
        const filePath =
          parentPath === "." ? fileName : `${parentPath}/${fileName}`;
        // Remove leading ./ if present
        const normalizedPath = filePath.replace(/^\.\//, "");
        await project.writeFile(normalizedPath, "");
        // Refresh the parent directory
        if (parentPath === ".") {
          await refreshFileTree();
        } else {
          await handleDirectoryExpand({
            fullName: parentPath,
            name: parentPath.split("/").pop() || parentPath,
            type: "directory",
            children: [],
          });
        }
        return true;
      } catch (e: any) {
        setError(`Error creating file: ${e.message}`);
        return false;
      }
    },
    [project, refreshFileTree, handleDirectoryExpand],
  );

  const createFolder = useCallback(
    async (parentPath: string, folderName: string): Promise<boolean> => {
      if (!project) {
        setError("Project not initialized");
        return false;
      }

      try {
        const folderPath =
          parentPath === "." ? folderName : `${parentPath}/${folderName}`;
        // Remove leading ./ if present
        const normalizedPath = folderPath.replace(/^\.\//, "");
        await project.mkdir(normalizedPath, { recursive: true });
        // Refresh the parent directory
        if (parentPath === ".") {
          await refreshFileTree();
        } else {
          await handleDirectoryExpand({
            fullName: parentPath,
            name: parentPath.split("/").pop() || parentPath,
            type: "directory",
            children: [],
          });
        }
        return true;
      } catch (e: any) {
        setError(`Error creating folder: ${e.message}`);
        return false;
      }
    },
    [project, refreshFileTree, handleDirectoryExpand],
  );

  const deleteItem = useCallback(
    async (item: FileTreeNode): Promise<boolean> => {
      if (!project) {
        setError("Project not initialized");
        return false;
      }

      try {
        const normalizedPath = item.fullName.replace(/^\.\//, "");
        if (item.type === "directory") {
          await project.rmdir(normalizedPath, { recursive: true });
        } else {
          await project.rm(normalizedPath);
        }
        // Refresh the file tree
        await refreshFileTree();
        return true;
      } catch (e: any) {
        setError(`Error deleting ${item.type}: ${e.message}`);
        return false;
      }
    },
    [project, refreshFileTree],
  );

  return {
    fileTree,
    handleDirectoryExpand,
    setFileTree,
    refreshFileTree,
    createFile,
    createFolder,
    deleteItem,
    error,
  };
};
