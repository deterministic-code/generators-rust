import type { EmittedFile, ServiceTestsEmitConfig } from "@deterministic-code/generator-sdk/codegen/lib/service-tests-emit-types";
interface RustTestCandidate {
    name: string;
}
interface RustTestEmitOptions {
    packageName?: string;
    organizeByFeature?: boolean;
}
export declare const DEFAULT_EMIT_OPTIONS: {
    readonly packageName: "generated";
};
export declare function emitGenericServiceTest(_candidate: RustTestCandidate, _opts?: RustTestEmitOptions): EmittedFile | null;
export declare const emit: (ctx: unknown) => Promise<unknown>;
export declare const createEmitter: () => {
    emit: (config: ServiceTestsEmitConfig) => EmittedFile[];
};
export {};
