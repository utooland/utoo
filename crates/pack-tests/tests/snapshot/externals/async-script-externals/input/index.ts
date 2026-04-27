import JSZip from 'jszip';

const zip = new JSZip();

zip;

const func = async () => {
    // @ts-ignore
    const _ = await import('lodash');
    console.log(Object.keys(_.default.omit({ a: 1 }, 'a')).length === 0);
};
func();