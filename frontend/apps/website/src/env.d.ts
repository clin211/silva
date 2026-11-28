import type { SiteData } from './lib/data';

declare global {
    namespace App {
        interface Locals {
            content: SiteData;
        }
    }
}
