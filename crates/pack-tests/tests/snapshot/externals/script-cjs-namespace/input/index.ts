const load = async () => {
    const jszip = await import('jszip');
    console.log(jszip.default, jszip.version);
};

load();
