
export async function Test() {
    const module = await import('./a');
    return module;
} 

