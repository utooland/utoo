import React, { useEffect, useState } from "react";
import {
  getConfig,
  REGISTRIES,
  RegistryKey,
  saveConfig,
  UtooConfig,
} from "../services/utooService";

interface ConfigPanelProps {
  isOpen: boolean;
  onClose: () => void;
  onConfigChange?: (config: UtooConfig) => void;
}

export const ConfigPanel: React.FC<ConfigPanelProps> = ({
  isOpen,
  onClose,
  onConfigChange,
}) => {
  const [config, setConfig] = useState<UtooConfig>(getConfig());
  const [showCustomRegistry, setShowCustomRegistry] = useState(
    config.registry !== "npmmirror" && config.registry !== "npm",
  );

  useEffect(() => {
    if (isOpen) {
      setConfig(getConfig());
    }
  }, [isOpen]);

  const handleRegistryChange = (value: string) => {
    if (value === "custom") {
      setShowCustomRegistry(true);
      setConfig((prev) => ({ ...prev, registry: "custom" as RegistryKey }));
    } else {
      setShowCustomRegistry(false);
      setConfig((prev) => ({
        ...prev,
        registry: value as RegistryKey,
        customRegistry: "",
      }));
    }
  };

  const handleSave = () => {
    saveConfig(config);
    onConfigChange?.(config);
    onClose();
  };

  if (!isOpen) return null;

  return (
    <div
      style={{
        position: "fixed",
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        backgroundColor: "rgba(0, 0, 0, 0.5)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 1000,
      }}
      onClick={onClose}
    >
      <div
        style={{
          backgroundColor: "#fff",
          borderRadius: "0.5rem",
          padding: "1.5rem",
          minWidth: "400px",
          maxWidth: "500px",
          boxShadow: "0 4px 6px rgba(0, 0, 0, 0.1)",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <h2
          style={{
            margin: "0 0 1.5rem 0",
            fontSize: "1.25rem",
            fontWeight: 600,
            color: "#1f2937",
          }}
        >
          Settings
        </h2>

        {/* Registry Selection */}
        <div style={{ marginBottom: "1.25rem" }}>
          <label
            style={{
              display: "block",
              marginBottom: "0.5rem",
              fontWeight: 500,
              color: "#374151",
            }}
          >
            Registry
          </label>
          <select
            value={showCustomRegistry ? "custom" : config.registry}
            onChange={(e) => handleRegistryChange(e.target.value)}
            style={{
              width: "100%",
              padding: "0.5rem",
              borderRadius: "0.375rem",
              border: "1px solid #d1d5db",
              fontSize: "0.875rem",
              backgroundColor: "#fff",
            }}
          >
            <option value="npmmirror">
              npmmirror ({REGISTRIES.npmmirror})
            </option>
            <option value="npm">npm ({REGISTRIES.npm})</option>
            <option value="custom">Custom...</option>
          </select>

          {showCustomRegistry && (
            <input
              type="text"
              placeholder="https://your-registry.com"
              value={config.customRegistry}
              onChange={(e) =>
                setConfig((prev) => ({
                  ...prev,
                  customRegistry: e.target.value,
                }))
              }
              style={{
                width: "100%",
                padding: "0.5rem",
                borderRadius: "0.375rem",
                border: "1px solid #d1d5db",
                fontSize: "0.875rem",
                marginTop: "0.5rem",
                boxSizing: "border-box",
              }}
            />
          )}
        </div>

        {/* Concurrent Downloads */}
        <div style={{ marginBottom: "1.5rem" }}>
          <label
            style={{
              display: "block",
              marginBottom: "0.5rem",
              fontWeight: 500,
              color: "#374151",
            }}
          >
            Max Concurrent Downloads: {config.maxConcurrentDownloads}
          </label>
          <input
            type="range"
            min="1"
            max="50"
            value={config.maxConcurrentDownloads}
            onChange={(e) =>
              setConfig((prev) => ({
                ...prev,
                maxConcurrentDownloads: parseInt(e.target.value, 10),
              }))
            }
            style={{
              width: "100%",
            }}
          />
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              fontSize: "0.75rem",
              color: "#6b7280",
            }}
          >
            <span>1</span>
            <span>25</span>
            <span>50</span>
          </div>
        </div>

        {/* Buttons */}
        <div
          style={{
            display: "flex",
            justifyContent: "flex-end",
            gap: "0.5rem",
          }}
        >
          <button
            onClick={onClose}
            style={{
              padding: "0.5rem 1rem",
              borderRadius: "0.375rem",
              border: "1px solid #d1d5db",
              backgroundColor: "#fff",
              fontSize: "0.875rem",
              cursor: "pointer",
            }}
          >
            Cancel
          </button>
          <button
            onClick={handleSave}
            style={{
              padding: "0.5rem 1rem",
              borderRadius: "0.375rem",
              border: "none",
              backgroundColor: "#2563eb",
              color: "#fff",
              fontSize: "0.875rem",
              fontWeight: 500,
              cursor: "pointer",
            }}
          >
            Save
          </button>
        </div>
      </div>
    </div>
  );
};
