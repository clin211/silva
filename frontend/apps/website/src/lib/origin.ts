import type { APIContext } from 'astro';

/** Canonical links use the configured public origin, including behind an internal reverse proxy. */
export function siteOrigin(context: Pick<APIContext, 'site' | 'url'>) {
    return new URL(process.env.SITE_URL || context.site || context.url.origin);
}
