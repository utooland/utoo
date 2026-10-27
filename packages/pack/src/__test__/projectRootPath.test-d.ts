import type { projectFactory } from "../core/project";
import type { ProjectOptions } from "../core/types";

declare const options: ProjectOptions;

// Existing consumers can keep reading a required string from ProjectOptions.
export const rootPath: string = options.rootPath;

// Only project creation accepts an omitted root for automatic discovery.
declare const withoutRoot: Omit<Required<ProjectOptions>, "rootPath">;
export const creationOptions: Parameters<ReturnType<typeof projectFactory>>[0] =
  withoutRoot;
