import React, { useEffect, useState } from "react";

interface Post {
  userId: number;
  id: number;
  title: string;
  body: string;
}

export function App() {
  const [post, setPost] = useState<Post | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [endpoint, setEndpoint] = useState<"/api" | "/placeholder">("/api");

  useEffect(() => {
    setLoading(true);
    setError(null);
    fetch(`${endpoint}/posts/1`)
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(r.statusText))))
      .then((data: Post) => {
        setPost(data);
        setLoading(false);
      })
      .catch((e) => {
        setError(e instanceof Error ? e.message : String(e));
        setLoading(false);
      });
  }, [endpoint]);

  return (
    <div
      style={{
        fontFamily: "system-ui",
        maxWidth: 640,
        margin: "2rem auto",
        padding: "0 1rem",
      }}
    >
      <h1>devServer.proxy Example</h1>
      <p style={{ color: "#666" }}>
        In dev, requests to <code>/api</code> and <code>/placeholder</code> are
        proxied to <code>https://jsonplaceholder.typicode.com</code> via{" "}
        <code>devServer.proxy</code> (Hono), so no CORS is needed.
      </p>

      <div style={{ marginBottom: "1rem" }}>
        <label>
          <input
            type="radio"
            name="path"
            checked={endpoint === "/api"}
            onChange={() => setEndpoint("/api")}
          />{" "}
          <code>/api</code> (pathRewrite: ^/api → "")
        </label>
        <br />
        <label>
          <input
            type="radio"
            name="path"
            checked={endpoint === "/placeholder"}
            onChange={() => setEndpoint("/placeholder")}
          />{" "}
          <code>/placeholder</code> (multiple contexts, same target)
        </label>
      </div>

      {loading && <p>Loading…</p>}
      {error && <p style={{ color: "crimson" }}>Error: {error}</p>}
      {post && !loading && (
        <article
          style={{ border: "1px solid #eee", borderRadius: 8, padding: "1rem" }}
        >
          <h2 style={{ marginTop: 0 }}>{post.title}</h2>
          <p style={{ color: "#444" }}>{post.body}</p>
          <small>
            Post #{post.id} (userId: {post.userId})
          </small>
        </article>
      )}
    </div>
  );
}
