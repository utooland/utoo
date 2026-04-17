import { createUser, deleteUser } from "./actions";

async function main() {
  const user = await createUser("Alice", "alice@example.com");
  console.log("Created user:", user);

  await deleteUser(user.id);
  console.log("Deleted user");
}

main();
