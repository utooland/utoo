const data = await fetch("/api/config").then((r) => r.json());

export const config = data;
export const version = "1.0.0";
