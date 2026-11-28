// 同域部署用当前 origin，Vite 开发时通过 Compose 网关访问真实发布内容。
export const websiteUrl =
    import.meta.env.VITE_WEBSITE_URL ||
    (import.meta.env.DEV ? 'http://localhost:4321' : window.location.origin);
