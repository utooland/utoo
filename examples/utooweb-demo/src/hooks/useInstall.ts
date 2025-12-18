import { Project as UtooProject } from "@utoo/web";
import { useEffect, useState } from "react";
import {
  hasPackageLock,
  installDependencies,
  UtooConfig,
  updateDependencies,
} from "../services/utooService";

export const useInstall = (
  project: UtooProject | null,
  config?: UtooConfig | null,
) => {
  const [isInstalling, setIsInstalling] = useState(false);
  const [isUpdating, setIsUpdating] = useState(false);
  const [hasLock, setHasLock] = useState(false);
  const [error, setError] = useState<string>("");

  useEffect(() => {
    const checkLock = async () => {
      if (project) {
        const exists = await hasPackageLock(project);
        setHasLock(exists);
      }
    };
    checkLock();
  }, [project]);

  const handleInstall = async () => {
    if (!project) {
      setError("Project not initialized");
      return;
    }

    setIsInstalling(true);
    setError("");

    try {
      await installDependencies(project, hasLock, config || undefined);
      setHasLock(true);
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e);
      setError(`Installation failed: ${errorMessage}`);
    } finally {
      setIsInstalling(false);
    }
  };

  const handleUpdate = async () => {
    if (!project) {
      setError("Project not initialized");
      return;
    }

    setIsUpdating(true);
    setError("");

    try {
      await updateDependencies(project, config || undefined);
      setHasLock(true);
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e);
      setError(`Update failed: ${errorMessage}`);
    } finally {
      setIsUpdating(false);
    }
  };

  return {
    isInstalling,
    isUpdating,
    handleInstall,
    handleUpdate,
    error,
    hasLock,
  };
};
