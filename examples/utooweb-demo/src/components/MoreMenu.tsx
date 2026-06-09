import React, { useEffect, useRef, useState } from "react";

export interface MenuItem {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
  testId?: string;
}

interface MoreMenuProps {
  items: MenuItem[];
  disabled?: boolean;
  testId?: string;
}

export const MoreMenu: React.FC<MoreMenuProps> = ({
  items,
  disabled,
  testId,
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };

    if (isOpen) {
      document.addEventListener("mousedown", handleClickOutside);
    }
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [isOpen]);

  return (
    <div
      data-testid={testId}
      ref={menuRef}
      style={{ position: "relative", display: "inline-block" }}
    >
      <button
        data-testid={testId ? `${testId}-button` : undefined}
        onClick={() => setIsOpen(!isOpen)}
        disabled={disabled}
        style={{
          padding: "0.25rem 0.75rem",
          borderRadius: "0.375rem",
          border: "none",
          fontSize: "0.875rem",
          background: disabled ? "#d1d5db" : "#6b7280",
          color: "#fff",
          fontWeight: 500,
          cursor: disabled ? "not-allowed" : "pointer",
          transition: "background 0.2s",
          marginLeft: "0.5rem",
        }}
      >
        More ▾
      </button>

      {isOpen && (
        <div
          data-testid={testId ? `${testId}-menu` : undefined}
          style={{
            position: "absolute",
            top: "100%",
            right: 0,
            marginTop: "0.25rem",
            backgroundColor: "#fff",
            borderRadius: "0.375rem",
            boxShadow: "0 4px 6px rgba(0, 0, 0, 0.1)",
            border: "1px solid #e5e7eb",
            minWidth: "140px",
            zIndex: 100,
          }}
        >
          {items.map((item, index) => (
            <button
              key={index}
              data-testid={item.testId}
              onClick={() => {
                item.onClick();
                setIsOpen(false);
              }}
              disabled={item.disabled}
              style={{
                display: "block",
                width: "100%",
                padding: "0.5rem 1rem",
                border: "none",
                background: "transparent",
                textAlign: "left",
                fontSize: "0.875rem",
                cursor: item.disabled ? "not-allowed" : "pointer",
                color: item.disabled
                  ? "#9ca3af"
                  : item.danger
                    ? "#ef4444"
                    : "#374151",
                borderBottom:
                  index < items.length - 1 ? "1px solid #e5e7eb" : "none",
              }}
              onMouseEnter={(e) => {
                if (!item.disabled) {
                  e.currentTarget.style.backgroundColor = "#f3f4f6";
                }
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.backgroundColor = "transparent";
              }}
            >
              {item.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
};
