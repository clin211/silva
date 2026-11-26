import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const pkgDir = resolve(import.meta.dirname, '..');
const specPath = resolve(pkgDir, '../../../server/apps/api/openapi.json');
const committedPath = resolve(pkgDir, 'src/schema.d.ts');
const tempDir = mkdtempSync(join(tmpdir(), 'silva-openapi-'));
const tempOut = join(tempDir, 'schema.d.ts');

try {
    execFileSync('pnpm', ['exec', 'openapi-typescript', specPath, '-o', tempOut], {
        cwd: pkgDir,
        stdio: 'pipe',
    });
    const expected = readFileSync(tempOut, 'utf8');
    const actual = readFileSync(committedPath, 'utf8');
    if (expected !== actual) {
        console.error(
            'OpenAPI client types are out of sync with server/apps/api/openapi.json. Run `pnpm generate:api`.',
        );
        process.exit(1);
    }
    console.log('OpenAPI client types are up to date.');
} finally {
    rmSync(tempDir, { recursive: true, force: true });
}
