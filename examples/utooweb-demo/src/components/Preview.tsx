import React, { useMemo } from "react";

interface PreviewProps {
    url: string;
}

export const Preview = ({ url }: PreviewProps) => {
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
                {url ? (
                    <iframe
                        src={url}
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
        [url],
    );

    return previewDisplay;
};
