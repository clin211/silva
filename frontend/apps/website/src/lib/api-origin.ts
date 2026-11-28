export function publicApiOrigin() {
    return new URL(process.env.SILVA_PUBLIC_API_ORIGIN || 'http://127.0.0.1:8088');
}
