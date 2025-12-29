export async function importUrl(url: string) {
    return import(`${url}`).then((module) => module.default);
}

export async function requireStat(name: string) {
    return require(name);
}
