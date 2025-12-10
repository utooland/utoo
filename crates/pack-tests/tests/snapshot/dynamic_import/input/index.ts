export function requireStat(name: string) {
    return require(require.resolve(`${name}/package.json'`))
}

export async function importUrl(url: string) {
    return import(`${url}`).then((module) => module.default);
}