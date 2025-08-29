import React from "react";

interface PreviewProps {
    url: string;
}

export const Preview = ({ url }: PreviewProps) => {
    return url ? (
        <iframe
            src={url}
            title="preview"
            style={{
                width: "100%",
                height: "100%",
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
    );
};
