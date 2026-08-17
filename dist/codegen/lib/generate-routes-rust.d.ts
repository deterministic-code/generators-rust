import { type NamesForOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
import type { GeneratedFile, RoutesGenerateConfig } from "@deterministic-code/generator-sdk/codegen/lib/routes-generate-types";
interface Field {
    name: string;
    type: string;
    isNullable?: boolean;
    hasDefault?: boolean;
}
interface Enrichment {
    targetTable: string;
    fkColumn: string;
    newField: string;
    prefix?: string;
}
interface EagerChild {
    fieldName: string;
    fkColumn: string;
    childTable: string;
    kind?: string;
}
interface PrimaryKey {
    column: string;
    idType?: string;
    rustType?: string;
}
interface RustGenerateOptions extends NamesForOptions {
    requiredFields?: Field[];
    allFields?: Field[];
    useOptimisticConcurrency?: boolean;
}
interface RustRouteCandidate {
    name: string;
    primaryKey?: PrimaryKey;
    fields?: Field[];
    enrichments?: Enrichment[];
    eagerWriteChildren?: EagerChild[];
    byFields?: unknown;
    datasourceType?: string;
    optimisticConcurrency?: boolean;
}
export declare function generateReadOnlyRouter(candidate: RustRouteCandidate, options?: RustGenerateOptions): GeneratedFile;
export declare function generateCrudRouter(candidate: RustRouteCandidate, options?: RustGenerateOptions): GeneratedFile;
export declare function generateCustomRouteStub(): null;
interface WiringRouter {
    name: string;
    enrichments?: Enrichment[];
    readOnly?: boolean;
}
interface AppWiringInput {
    routers: WiringRouter[];
}
/** The generated app-wiring aggregator: `compose_router(ctx)` builds each generated service facade from
 * the ComposeContext (each pulls its composed runtime stack) and merges the generated router that routes
 * to it — the single live source of truth. The runtime's RouteComposer hook calls this. */
export declare function generateAppWiring(wiring: AppWiringInput, options?: RustGenerateOptions): GeneratedFile;
/** Catalog `routes` step (rust). */
export declare const generate: (ctx: unknown) => Promise<unknown>;
export declare const createGenerator: () => {
    generate: (config: RoutesGenerateConfig) => GeneratedFile[];
};
export {};
