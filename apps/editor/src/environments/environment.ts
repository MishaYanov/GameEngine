// Production is the safe default, including builds without a configuration flag.
export const environment = {
    name: 'production',
    toolsEnabled: false,
    defaultRoute: 'setup',
} as const;
