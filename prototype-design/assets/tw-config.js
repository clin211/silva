/* Shared Tailwind (CDN) config — load right after https://cdn.tailwindcss.com */
tailwind.config = {
  theme: {
    extend: {
      colors: {
        paper: { DEFAULT: '#F6F4EF', 2: '#EEEAE2', 3: '#E5E0D5' },
        ink: { DEFAULT: '#1D2330', 2: '#3A404C', soft: '#6A6F79' },
        rule: '#DCD6CB',
        accent: { DEFAULT: '#27467A', 2: '#1B3360', tint: '#E3E8F1' },
        // admin neutrals
        ui: { bg: '#F4F5F7', panel: '#FFFFFF', line: '#E2E4E9', text: '#1D2330', sub: '#5E6470' },
        ok: { DEFAULT: '#1F6B4A', tint: '#E2F0E8' },
        warn: { DEFAULT: '#8A5A00', tint: '#F8EDD5' },
        bad: { DEFAULT: '#A3261F', tint: '#F7E1DF' },
      },
      fontFamily: {
        serif: ['"Noto Serif SC"', 'Georgia', 'serif'],
        sans: ['"Noto Sans SC"', 'system-ui', '-apple-system', 'sans-serif'],
        code: ['"JetBrains Mono"', 'ui-monospace', 'monospace'],
        // public site (v2, ui-ux-pro-max: Bento + monochrome/blue)
        display: ['"Space Grotesk"', '"Noto Sans SC"', 'system-ui', 'sans-serif'],
        body: ['Inter', '"Noto Sans SC"', 'system-ui', '-apple-system', 'sans-serif'],
        mono: ['"JetBrains Mono"', 'ui-monospace', 'SFMono-Regular', 'monospace'],
      },
      maxWidth: { site: '1200px', prose: '720px' },
    },
  },
};
