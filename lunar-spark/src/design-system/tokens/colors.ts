export const colors = {
  // Backgrounds
  background: {
    primary: '#0A0A0A',
    secondary: '#171717',
    tertiary: '#262626',
    elevated: '#1F1F1F',
  },

  // Text/Foreground
  foreground: {
    primary: '#FFFFFF',
    secondary: '#A3A3A3',
    tertiary: '#737373',
    disabled: '#525252',
  },

  // Borders
  border: {
    default: '#262626',
    subtle: '#1F1F1F',
    strong: '#404040',
  },

  // Brand/Accent - Purple
  accent: {
    default: '#A855F7',
    hover: '#9333EA',
    light: '#C084FC',
    dark: '#7E22CE',
  },

  // Status
  success: {
    default: '#22C55E',
    light: '#4ADE80',
    dark: '#16A34A',
  },
  warning: {
    default: '#F59E0B',
    light: '#FBBF24',
    dark: '#D97706',
  },
  error: {
    default: '#EF4444',
    light: '#F87171',
    dark: '#DC2626',
  },
  info: {
    default: '#3B82F6',
    light: '#60A5FA',
    dark: '#2563EB',
  },

  // Wallet-specific
  wallet: {
    shielded: '#A855F7',
    unshielded: '#3B82F6',
    dust: '#EAB308',
  },
} as const;

export type Colors = typeof colors;
