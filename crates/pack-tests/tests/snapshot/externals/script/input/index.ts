const func = async () => {
    // @ts-ignore
    const _ = await import('lodash');
    console.log(Object.keys(_.default.omit({ a: 1 }, 'a')).length === 0);
    const esm = await import('esm-script');
    console.log(esm.default, esm.named);
};
func();
