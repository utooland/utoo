import React, { forwardRef, useImperativeHandle, useRef } from "react";

interface PreviewProps {
  url: string;
  onIframeReady?: (iframe: HTMLIFrameElement) => { close: () => void } | null;
}

export const Preview = forwardRef(
  ({ url, onIframeReady }: PreviewProps, ref) => {
    const iframeRef = useRef<HTMLIFrameElement>(null);
    const hmrClientRef = useRef<{ close: () => void } | null>(null);

    useImperativeHandle(ref, () => ({
      reload: () => {
        if (iframeRef.current) {
          iframeRef.current.contentWindow?.location.reload();
        }
      },
    }));

    // Listen for HMR ready message from iframe
    React.useEffect(() => {
      const iframe = iframeRef.current;
      if (!iframe || !onIframeReady || !url) return;

      const handleMessage = (event: MessageEvent) => {
        // Check if this is an hmr-ready message from our iframe
        if (
          event.data?.type === "hmr-ready" &&
          event.source === iframe.contentWindow
        ) {
          // Disconnect previous HMR client if exists
          if (hmrClientRef.current) {
            hmrClientRef.current.close();
            hmrClientRef.current = null;
          }

          // Connect HMR client via callback
          const client = onIframeReady(iframe);
          if (client) {
            hmrClientRef.current = client;
            console.log("[Preview] HMR connected");
          }
        }
      };

      window.addEventListener("message", handleMessage);

      return () => {
        window.removeEventListener("message", handleMessage);
        if (hmrClientRef.current) {
          hmrClientRef.current.close();
          hmrClientRef.current = null;
        }
      };
    }, [onIframeReady, url]);

    return url ? (
      <iframe
        data-testid="preview-iframe"
        ref={iframeRef}
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
        data-testid="preview-empty-state"
        style={{ color: "#9ca3af", textAlign: "center", marginTop: "2rem" }}
      >
        No index.html to preview
      </div>
    );
  },
);
