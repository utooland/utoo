"use server";

export async function createUser(name: string, email: string) {
  // This runs on the server
  const user = { id: Math.random().toString(36), name, email };
  return user;
}

export async function deleteUser(id: string) {
  // This runs on the server
  console.log(`Deleting user ${id}`);
}
