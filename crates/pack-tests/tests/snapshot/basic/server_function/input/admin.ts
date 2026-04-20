"use server";

export async function createUser(name: string, role: string) {
  // Admin createUser implementation
  console.log(`Creating admin user ${name} with role ${role}`);
  return { id: "admin-" + Math.random().toString(36), name, role };
}
