import React, { useState } from "react";
import { createUser, deleteUser } from "./actions";

export default function App() {
  const [status, setStatus] = useState<string>("idle");
  const [userId, setUserId] = useState<string | null>(null);

  const handleCreate = async () => {
    setStatus("creating...");
    const user = await createUser("Alice", "alice@example.com");
    setUserId(user.id);
    setStatus(`Created user: ${user.name} (${user.id})`);
  };

  const handleDelete = async () => {
    if (!userId) return;
    setStatus("deleting...");
    await deleteUser(userId);
    setUserId(null);
    setStatus("User deleted");
  };

  return (
    <div style={{ padding: 20, fontFamily: "sans-serif" }}>
      <h1>Server Functions Example</h1>
      <p>Status: {status}</p>
      <button onClick={handleCreate}>Create User</button>
      <button onClick={handleDelete} disabled={!userId}>
        Delete User
      </button>
    </div>
  );
}
