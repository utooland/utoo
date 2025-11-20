export interface Args {
    experimentId?: string;
    [key: string]: any;
}
export interface Target {
    id: number;
    name: string;
    codeName?: string;
    nodeDefId?: number;
    type: 'node' | 'group';
    [key: string]: any;
}