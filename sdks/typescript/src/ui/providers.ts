export interface ProviderBrand {
  id: string;
  name: string;
  /** Button surface and text color (brand rules where they exist). */
  bg: string;
  fg: string;
  border?: string;
  /** Inline SVG, 24x24, `currentColor` unless the brand needs its own colors. */
  icon: string;
}

const svg = (body: string): string =>
  `<svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false">${body}</svg>`;

export const genericIcon = svg('<path fill="currentColor" d="M10 17l5-5-5-5v3H3v4h7zM13 3h8v18h-8v-2h6V5h-6z"/>');

export const providerBrands: Record<string, ProviderBrand> = {
  google: {
    id: "google",
    name: "Google",
    bg: "#ffffff",
    fg: "#3c4043",
    border: "#dadce0",
    icon: svg(
      '<path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"/><path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"/><path fill="#FBBC05" d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"/><path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"/>',
    ),
  },
  github: {
    id: "github",
    name: "GitHub",
    bg: "#24292f",
    fg: "#ffffff",
    icon: svg(
      '<path fill="currentColor" d="M12 .3a12 12 0 00-3.8 23.4c.6.1.8-.3.8-.6v-2c-3.3.7-4-1.6-4-1.6-.6-1.4-1.4-1.8-1.4-1.8-1-.7.1-.7.1-.7 1.2.1 1.8 1.200 1.8 1.200 1 1.800 2.800 1.300 3.500 1 .1-.8.4-1.300.7-1.600-2.700-.3-5.500-1.300-5.500-5.900 0-1.300.5-2.400 1.200-3.200-.1-.3-.5-1.500.1-3.200 0 0 1-.3 3.300 1.200a11.500 11.500 0 016 0C17.300 4.700 18.300 5 18.300 5c.7 1.700.3 2.900.1 3.200.8.8 1.200 1.900 1.200 3.200 0 4.600-2.800 5.600-5.500 5.900.4.400.8 1.100.8 2.200v3.300c0 .3.2.7.8.6A12 12 0 0012 .3"/>',
    ),
  },
  apple: {
    id: "apple",
    name: "Apple",
    bg: "#000000",
    fg: "#ffffff",
    border: "#000000",
    icon: svg(
      '<path fill="currentColor" d="M16.4 12.6c0-2.300 1.900-3.400 2-3.500-1.100-1.600-2.800-1.800-3.400-1.800-1.400-.1-2.800.9-3.500.9s-1.800-.8-3-.8c-1.500 0-3 .9-3.800 2.300-1.600 2.800-.4 7 1.200 9.300.8 1.100 1.700 2.400 2.900 2.300 1.200 0 1.600-.8 3-.8s1.800.8 3 .7c1.300 0 2.100-1.100 2.800-2.300.9-1.300 1.200-2.500 1.300-2.600-.1 0-2.500-1-2.500-3.700zM14.200 5.800c.6-.8 1.100-1.800.9-2.800-.9.040-2 .6-2.600 1.400-.6.700-1.100 1.700-.9 2.700 1 .1 2-.5 2.600-1.300z"/>',
    ),
  },
  microsoft: {
    id: "microsoft",
    name: "Microsoft",
    bg: "#2f2f2f",
    fg: "#ffffff",
    icon: svg(
      '<path fill="#F25022" d="M3 3h8.500v8.500H3z"/><path fill="#7FBA00" d="M12.500 3H21v8.500h-8.500z"/><path fill="#00A4EF" d="M3 12.500h8.500V21H3z"/><path fill="#FFB900" d="M12.500 12.500H21V21h-8.500z"/>',
    ),
  },
  facebook: {
    id: "facebook",
    name: "Facebook",
    bg: "#1877f2",
    fg: "#ffffff",
    icon: svg('<path fill="currentColor" d="M14 8V6.500c0-.7.200-1 1.100-1H17V2h-2.600C11.400 2 10 3.700 10 6.200V8H8v3.500h2V22h4V11.500h2.700L17 8z"/>'),
  },
  discord: {
    id: "discord",
    name: "Discord",
    bg: "#5865f2",
    fg: "#ffffff",
    icon: svg(
      '<path fill="currentColor" fill-rule="evenodd" d="M7 5c2-.8 3.500-1 5-1s3 .2 5 1c1.500 2.500 2.500 5.500 3 9-1.300 1.200-2.600 1.900-4 2.400l-.9-1.500c.5-.2 1-.4 1.400-.7-.3-.2-.6-.4-.9-.5-2 .9-4.800.9-6.800 0-.3.1-.6.3-.9.5.4.3.9.5 1.400.7L7 16.400C5.600 15.900 4.300 15.200 3 14c.5-3.500 1.500-6.500 4-9zm2.500 5.200a1.400 1.500 0 100 3 1.400 1.500 0 000-3zm5 0a1.400 1.500 0 100 3 1.400 1.500 0 000-3z"/>',
    ),
  },
  gitlab: {
    id: "gitlab",
    name: "GitLab",
    bg: "#fc6d26",
    fg: "#ffffff",
    icon: svg('<path fill="currentColor" d="M12 22L2.500 14.500 4.500 7l2.500 6h10l2.500-6 2 7.500z"/><path fill="currentColor" d="M7 13L9.500 4 12 13zM12 13l2.500-9L17 13z" opacity=".7"/>'),
  },
  x: {
    id: "x",
    name: "X",
    bg: "#000000",
    fg: "#ffffff",
    border: "#000000",
    icon: svg('<path fill="none" stroke="currentColor" stroke-width="2.200" stroke-linecap="round" d="M4.500 4.500l15 15M19.500 4.500l-15 15"/>'),
  },
  linkedin: {
    id: "linkedin",
    name: "LinkedIn",
    bg: "#0a66c2",
    fg: "#ffffff",
    icon: svg(
      '<path fill="currentColor" d="M4 9h3.500v11H4zM5.750 3.500a2 2 0 110 4 2 2 0 010-4zM10 9h3.300v1.500c.5-.9 1.700-1.800 3.400-1.800 3.400 0 4.300 2.200 4.300 5.100V20h-3.500v-5.400c0-1.300 0-2.900-1.800-2.900s-2 1.400-2 2.800V20H10z"/>',
    ),
  },
  twitch: {
    id: "twitch",
    name: "Twitch",
    bg: "#9146ff",
    fg: "#ffffff",
    icon: svg('<path fill="currentColor" fill-rule="evenodd" d="M4 3h17v11l-4 4h-4l-3 3v-3H4V3zm2 2v11h4v1.800L11.800 16H16l3-3V5H6zm4 2h2v5h-2V7zm5 0h2v5h-2V7z"/>'),
  },
  oidc: {
    id: "oidc",
    name: "OpenID Connect",
    bg: "#475569",
    fg: "#ffffff",
    icon: svg('<path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" d="M8 15a4 4 0 110-8 4 4 0 010 8zm4-4h9m-3 0v4m-3-4v3"/>'),
  },
};

const NEUTRAL: Omit<ProviderBrand, "id" | "name"> = { bg: "var(--a-surface)", fg: "var(--a-fg)", border: "var(--a-border)", icon: genericIcon };

/** Known brand, or a neutral button using the name the server sent. */
export function brandFor(id: string, serverName?: string): ProviderBrand {
  const known = providerBrands[id];
  if (known) return serverName ? { ...known, name: serverName } : known;
  return { ...NEUTRAL, id, name: serverName || id };
}
