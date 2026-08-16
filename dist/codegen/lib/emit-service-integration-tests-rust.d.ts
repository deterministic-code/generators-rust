import type { EmittedFile, ServiceTestsEmitConfig } from "@deterministic-code/generator-sdk/codegen/lib/service-tests-emit-types";
import type { IntegrationTestCandidate } from "@deterministic-code/generator-sdk/codegen/lib/service-integration-tests-emit-types";
interface RustEmitOptions {
    servicePath?: string | null;
    fileFormat?: string;
    datetime?: string;
}
export declare const DEFAULT_EMIT_OPTIONS: {
    readonly servicePath: null;
    readonly fileFormat: "Snake";
    readonly datetime: "string";
};
export declare function emitGenericServiceIntegrationTest(_candidate: IntegrationTestCandidate, _opts?: RustEmitOptions): EmittedFile | null;
export declare const emit: (ctx: unknown) => Promise<unknown>;
export declare const createEmitter: () => {
    emit: (config: ServiceTestsEmitConfig) => EmittedFile[];
};
export {};
