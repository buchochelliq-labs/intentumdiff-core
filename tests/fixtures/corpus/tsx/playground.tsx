interface User {
  id: number;
  name: string;
}

function getUser(id: number): User {
  return { id, name: "Alice" };
}

function formatName(user: User): string {
  return user.name.toUpperCase();
}
