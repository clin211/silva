import createClient, { type ClientOptions } from 'openapi-fetch';
import type { components, operations, paths } from './schema.d.ts';

export type { components, operations, paths };

export type Schemas = components['schemas'];
export type ApiClient = ReturnType<typeof createClient<paths>>;

export function createSilvaClient(options: ClientOptions = {}): ApiClient {
    return createClient<paths>({
        baseUrl: '',
        ...options,
    });
}
