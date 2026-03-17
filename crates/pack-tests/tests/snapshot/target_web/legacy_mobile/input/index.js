class LegacyBox {
  constructor(value) {
    this.value = value;
  }

  toJSON() {
    return {
      value: this.value,
    };
  }
}

export async function loadProfile(rawUser) {
  const lazy = await import("./lazy.js");
  const name = rawUser?.profile?.name ?? "guest";
  const tags = [...(rawUser?.tags ?? []), ...lazy.extraTags];
  const profile = {
    ...lazy.defaults,
    name,
    tags,
  };
  const box = new LegacyBox(profile);

  return box.toJSON();
}

loadProfile({
  profile: {
    name: "utoo",
  },
  tags: ["pack"],
}).then((profile) => {
  console.log(
    profile.value.name,
    profile.value.tags.join(","),
    profile.value.tags.includes("legacy"),
  );
});
