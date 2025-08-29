import { Project as UtooProject } from "@utoo/web";

export interface FileTreeNode {
  name: string;
  fullName: string;
  type: "directory" | "file";
  children: FileTreeNode[] | null;
}

export interface FileTreeItemProps {
  item: FileTreeNode;
  onFileClick: (filePath: string) => Promise<void>;
  onDirectoryExpand?: (parentItem: FileTreeNode) => Promise<void>;
}

export interface DirectoryExpandParams {
  fullName: string;
  name: string;
  type: "directory" | "file";
  children: FileTreeNode[] | null;
}

export interface ProjectFileItem {
  name: string;
  isDirectory: () => boolean;
}
