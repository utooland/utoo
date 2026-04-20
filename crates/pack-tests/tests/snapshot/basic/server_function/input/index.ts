import { createUser, deleteUser } from "./actions";
import { createUser as createAdminUser } from "./admin";

async function main() {
  const user = await createUser("Alice", "alice@example.com");
  console.log("Created user:", user);

  await deleteUser(user.id);
  console.log("Deleted user");

  const admin = await createAdminUser("Bob", "superadmin");
  console.log("Created admin user:", admin);
}

main();
