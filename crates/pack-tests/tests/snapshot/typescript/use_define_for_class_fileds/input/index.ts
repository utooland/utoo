export class BrowserFsProvider {
    private opfsFsProvider: any;
    constructor() {
        this.opFs = this.opfsFsProvider.fs;
    }
}