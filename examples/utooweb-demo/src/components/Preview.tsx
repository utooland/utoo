import type { Project as UtooProject } from "@utoo/web";
import React, {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
} from "react";

interface PreviewProps {
  url: string;
  project?: UtooProject | null;
}

export const Preview = forwardRef(({ url, project }: PreviewProps, ref) => {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const hmrClientRef = useRef<{ close: () => void } | null>(null);

  useImperativeHandle(ref, () => ({
    reload: () => {
      if (iframeRef.current) {
        iframeRef.current.contentWindow?.location.reload();
      }
    },
  }));

  // Connect HMR when iframe signals it's ready
  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe || !project || !url) return;

    // Check if project has HMR support
    const hmrServer = (
      project as unknown as {
        hmrServer?: {
          connectIframe: (
            iframe: HTMLIFrameElement,
          ) => { close: () => void } | null;
        };
      }
    ).hmrServer;
    if (!hmrServer) return;

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

        // Connect HMR client
        const client = hmrServer.connectIframe(iframe);
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
  }, [project, url]);

  return url ? (
    <iframe
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
    <div style={{ color: "#9ca3af", textAlign: "center", marginTop: "2rem" }}>
      No index.html to preview
    </div>
  );
});
