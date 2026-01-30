export function setTernContainer(container: HTMLElement) {
    console.log('Setting Tern container:', container);
    return container;
}

export function initializeApp(config: { theme: string }) {
    console.log('Initializing app with config:', config);
    return { success: true, config };
}

export const APP_VERSION = '1.0.0';

export default {
    setTernContainer,
    initializeApp,
    version: APP_VERSION
};
