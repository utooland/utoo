import { useState, useEffect, useRef } from "react";
import { Project as UtooProject } from "@utoo/web";
import { initializeProject } from "../services/utooService";

export const useUtooProject = () => {
    const [project, setProject] = useState<UtooProject | null>(null);
    const [isLoading, setIsLoading] = useState(true);
    const [error, setError] = useState("");
    const initStarted = useRef(false);

    useEffect(() => {
        if (initStarted.current) return;
        initStarted.current = true;

        const init = async () => {
            setIsLoading(true);
            try {
                const projectInstance = await initializeProject();
                setProject(projectInstance);
            } catch (e) {
                const errorMessage = e instanceof Error ? e.message : String(e);
                setError(`Initialization failed: ${errorMessage}`);
            } finally {
                setIsLoading(false);
            }
        };

        init();
    }, []);

    return { project, isLoading, error };
};
