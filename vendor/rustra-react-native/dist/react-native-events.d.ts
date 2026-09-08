export type RustraEventNative = {
    onEvent?(name: string, callback: (payloadJson: string) => void): void;
    offEvent?(name: string): void;
};
export type RustraChannelNative = {
    createChannel?(callback: (payloadJson: string) => void): number;
    dropChannel?(handle: number): boolean;
};
export declare function createChannel(callback: (payload: unknown) => void, native?: RustraChannelNative): {
    readonly handle: number;
    close(): boolean;
};
type SubscribeOptions = {
    allowMissingNative?: boolean;
};
export declare function subscribeEvent(name: string, cb: (payload: unknown) => void, options?: SubscribeOptions): () => void;
export {};
//# sourceMappingURL=react-native-events.d.ts.map