"use server";

export async function createUser(name: string, email: string) {
  // This code runs on the server — it will be replaced with a
  // client-side proxy that calls the transport function.
  const user = { id: Math.random().toString(36).slice(2), name, email };
  console.log("[server] Created user:", user);
  return user;
}

export async function deleteUser(id: string) {
  // This code runs on the server
  console.log(`[server] Deleting user ${id}`);
}
